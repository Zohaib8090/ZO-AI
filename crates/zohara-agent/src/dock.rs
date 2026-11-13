//! The panel on the right of the window: the pinned Context panel, the terminals (which the AI can open and type into too),
//! and the built-in browser. Its edge can be dragged to change its width.

use gtk4 as gtk;
use gtk::{gio, glib};
use gtk::prelude::*;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;
use webkit6::prelude::*;
use zo_core::term::{self, Term};

use crate::{hbox, icon_button, vbox};

type Reply = mpsc::Sender<Result<String, String>>;

pub struct Dock {
    pub paned: gtk::Paned,
    root: gtk::Box,
    stack: gtk::Stack,
    tab_bar: gtk::Box,
    tab_buttons: RefCell<HashMap<&'static str, gtk::ToggleButton>>,
    pub context_body: gtk::Box,
    context_pinned: Cell<bool>,
    pub refresh_context: RefCell<Option<Rc<dyn Fn()>>>,
    width: Cell<i32>,
    grown: Cell<bool>,
    auto_hidden: Cell<bool>,
    // terminals
    term_tabs: gtk::Box,
    term_stack: gtk::Stack,
    term_views: RefCell<HashMap<u32, Rc<TermView>>>,
    // browser
    browser: RefCell<Option<Rc<Browser>>>,
    browser_box: gtk::Box,
}

struct TermView {
    term: Arc,
    view: gtk::TextView,
    tab: gtk::Box,
    tab_btn: gtk::ToggleButton,
    tags: RefCell<HashMap<String, gtk::TextTag>>,
    seen: Cell<u64>,
    back: Cell<usize>,
    dirty: Cell<bool>,
}
type Arc = std::sync::Arc<Term>;

struct Browser {
    web: webkit6::WebView,
    url: gtk::Entry,
    pending_load: RefCell<Option<Reply>>,
}

fn config_flag(name: &str) -> std::path::PathBuf {
    zo_core::audit::config_dir().join(name)
}

impl Dock {
    pub fn new(main_col: &gtk::Box) -> Rc<Dock> {
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(main_col));
        paned.set_resize_start_child(true);
        paned.set_shrink_start_child(true);
        paned.set_resize_end_child(false);
        paned.set_shrink_end_child(false);
        paned.add_css_class("dock-paned");
        paned.set_hexpand(true);

        let root = vbox(0);
        root.add_css_class("dock");
        let head = hbox(4);
        head.add_css_class("dock-head");
        let tab_bar = hbox(2);
        tab_bar.set_hexpand(true);
        head.append(&tab_bar);
        let close = icon_button("window-close-symbolic", "Hide this panel", "icon-btn");
        head.append(&close);
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        root.append(&head);
        root.append(&stack);

        // context page
        let context_body = vbox(8);
        context_body.add_css_class("ctx-panel");
        context_body.set_margin_top(6);
        context_body.set_margin_bottom(12);
        context_body.set_margin_start(12);
        context_body.set_margin_end(12);
        let ctx_scroll = gtk::ScrolledWindow::new();
        ctx_scroll.set_child(Some(&context_body));
        ctx_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        stack.add_named(&ctx_scroll, Some("context"));

        // terminal page
        let term_page = vbox(0);
        let term_head = hbox(2);
        term_head.add_css_class("term-tabs");
        let term_tabs = hbox(2);
        let tabs_scroll = gtk::ScrolledWindow::new();
        tabs_scroll.set_child(Some(&term_tabs));
        tabs_scroll.set_hexpand(true);
        tabs_scroll.set_vscrollbar_policy(gtk::PolicyType::Never);
        tabs_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
        term_head.append(&tabs_scroll);
        let new_term = icon_button("list-add-symbolic", "New terminal", "icon-btn");
        term_head.append(&new_term);
        let term_stack = gtk::Stack::new();
        term_stack.set_vexpand(true);
        term_page.append(&term_head);
        term_page.append(&term_stack);
        stack.add_named(&term_page, Some("terminal"));

        // browser page
        let browser_box = vbox(0);
        stack.add_named(&browser_box, Some("browser"));

        let dock = Rc::new(Dock {
            paned: paned.clone(),
            root: root.clone(),
            stack,
            tab_bar,
            tab_buttons: RefCell::new(HashMap::new()),
            context_body,
            context_pinned: Cell::new(config_flag("context-pinned").exists()),
            refresh_context: RefCell::new(None),
            width: Cell::new(crate::theme::zs(440)),
            grown: Cell::new(false),
            auto_hidden: Cell::new(false),
            term_tabs,
            term_stack,
            term_views: RefCell::new(HashMap::new()),
            browser: RefCell::new(None),
            browser_box,
        });
        root.set_visible(false);
        paned.set_end_child(Some(&root));
        {
            let d = dock.clone();
            crate::theme::on_zoom(move |_| {
                d.width.set(crate::theme::zs(440));
                d.root.set_size_request(crate::theme::zs(300), -1);
            });
        }
        {
            let d = dock.clone();
            close.connect_clicked(move |_| d.hide());
        }
        {
            let d = dock.clone();
            new_term.connect_clicked(move |_| {
                let _ = term::open("", None, None, false);
                d.sync_terminals();
            });
        }

        // the AI's requests for the browser come in here (from the agent's thread)
        let (tx, rx) = mpsc::channel::<(String, Value, Reply)>();
        let tx = Mutex::new(tx);
        zo_core::uibridge::register(move |name, args| {
            let (rtx, rrx) = mpsc::channel();
            tx.lock().unwrap().send((name.to_string(), args.clone(), rtx)).map_err(|_| "The window is closed.".to_string())?;
            rrx.recv_timeout(Duration::from_secs(90)).map_err(|_| "The browser did not answer in time.".to_string())?
        });
        zo_core::term::enable_in_app();

        let d = dock.clone();
        glib::timeout_add_local(Duration::from_millis(120), move || {
            while let Ok((name, args, reply)) = rx.try_recv() {
                d.browser_request(&name, &args, reply);
            }
            d.tick();
            glib::ControlFlow::Continue
        });
        if dock.context_pinned.get() {
            dock.add_tab("context", "Context");
        }
        dock
    }

    // ------------------------------------------------------------ pages

    fn add_tab(self: &Rc<Self>, page: &'static str, label: &str) {
        if self.tab_buttons.borrow().contains_key(page) {
            return;
        }
        let b = gtk::ToggleButton::with_label(label);
        b.add_css_class("dock-tab");
        let d = self.clone();
        b.connect_clicked(move |_| d.select(page));
        self.tab_bar.append(&b);
        self.tab_buttons.borrow_mut().insert(page, b);
    }

    fn select(&self, page: &str) {
        self.stack.set_visible_child_name(page);
        for (name, b) in self.tab_buttons.borrow().iter() {
            b.set_active(*name == page);
        }
    }

    fn remove_tab(&self, page: &str) {
        if let Some(b) = self.tab_buttons.borrow_mut().remove(page) {
            self.tab_bar.remove(&b);
        }
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    /// Shows the panel (at the width it had, or a default one).
    pub fn show(self: &Rc<Self>, page: &'static str) {
        let label = match page {
            "context" => "Context",
            "terminal" => "Terminal",
            _ => "Browser",
        };
        self.add_tab(page, label);
        if page == "terminal" && self.term_views.borrow().is_empty() && term::list().is_empty() {
            let _ = term::open("", None, None, false);
            self.sync_terminals();
        }
        if page == "browser" {
            self.ensure_browser();
        }
        if !self.root.is_visible() {
            self.root.set_visible(true);
            let w = self.width.get();
            self.grown.set(false);
            let paned = self.paned.clone();
            glib::timeout_add_local_once(Duration::from_millis(120), move || {
                let total = paned.width();
                if total > 0 {
                    paned.set_position(total - w.min(total / 2).max(crate::theme::zs(280)).min(total - 200));
                }
            });
        }
        self.select(page);
    }

    pub fn hide(&self) {
        if self.root.is_visible() {
            let total = self.paned.width();
            if total > 0 {
                self.width.set((total - self.paned.position()).max(crate::theme::zs(300)));
            }
        }
        let w = self.width.get();
        self.root.set_visible(false);
        if self.grown.replace(false) {
            if let Some(win) = self.paned.root().and_downcast::<gtk::Window>() {
                if !win.is_maximized() && !win.is_fullscreen() {
                    win.set_default_size((win.width() - w).max(600), win.height());
                }
            }
        }
    }

    pub fn toggle(self: &Rc<Self>, page: &'static str) {
        if self.is_open() && self.stack.visible_child_name().as_deref() == Some(page) {
            self.hide();
        } else {
            self.show(page);
        }
    }

    // ------------------------------------------------------------ context

    pub fn context_pinned(&self) -> bool {
        self.context_pinned.get()
    }

    pub fn pin_context(self: &Rc<Self>, on: bool) {
        self.context_pinned.set(on);
        let f = config_flag("context-pinned");
        if on {
            let _ = std::fs::create_dir_all(zo_core::audit::config_dir());
            let _ = std::fs::write(&f, "1");
            self.show("context");
        } else {
            let _ = std::fs::remove_file(&f);
            self.remove_tab("context");
            if self.stack.visible_child_name().as_deref() == Some("context") {
                let other = self.tab_buttons.borrow().keys().next().copied();
                match other {
                    Some(p) => self.select(p),
                    None => self.hide(),
                }
            }
        }
    }

    pub fn refresh_context_now(&self) {
        if self.context_pinned.get() {
            let f = self.refresh_context.borrow().clone();
            if let Some(f) = f {
                f();
            }
        }
    }

    // ------------------------------------------------------------ terminals

    fn sync_terminals(self: &Rc<Self>) {
        let live = term::list();
        let mut views = self.term_views.borrow_mut();
        // gone ones
        let gone: Vec<u32> = views.keys().copied().filter(|id| !live.iter().any(|t| t.id == *id)).collect();
        for id in gone {
            if let Some(v) = views.remove(&id) {
                self.term_tabs.remove(&v.tab);
                self.term_stack.remove(&v.view.parent().unwrap_or_else(|| v.view.clone().upcast()));
            }
        }
        // new ones
        let mut opened_by_ai = false;
        for t in live {
            if views.contains_key(&t.id) {
                continue;
            }
            let v = self.make_view(&t);
            opened_by_ai |= t.by_ai;
            views.insert(t.id, v);
        }
        let any = !views.is_empty();
        drop(views);
        if opened_by_ai {
            self.show("terminal");
        }
        if any && self.term_stack.visible_child_name().is_none() {
            if let Some(first) = self.term_views.borrow().values().next() {
                self.pick_terminal(first.term.id);
            }
        }
    }

    fn pick_terminal(&self, id: u32) {
        let views = self.term_views.borrow();
        for (vid, v) in views.iter() {
            v.tab_btn.set_active(*vid == id);
        }
        self.term_stack.set_visible_child_name(&format!("t{id}"));
        if let Some(v) = views.get(&id) {
            v.view.grab_focus();
            v.dirty.set(true);
        }
    }

    fn make_view(self: &Rc<Self>, t: &Arc) -> Rc<TermView> {
        let view = gtk::TextView::new();
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_monospace(true);
        view.set_wrap_mode(gtk::WrapMode::None);
        view.set_focusable(true);
        view.add_css_class("term");
        view.set_left_margin(6);
        view.set_top_margin(4);
        let holder = vbox(0);
        holder.append(&view);
        holder.add_css_class("term");
        holder.set_vexpand(true);
        view.set_vexpand(true);
        view.set_hexpand(true);
        self.term_stack.add_named(&holder, Some(&format!("t{}", t.id)));

        let tab = hbox(0);
        tab.add_css_class("term-tab");
        let tab_btn = gtk::ToggleButton::with_label(&format!("{}{}", if t.by_ai { "AI · " } else { "" }, t.name()));
        tab_btn.add_css_class("flat");
        let x = icon_button("window-close-symbolic", "Close this terminal", "icon-btn");
        tab.append(&tab_btn);
        tab.append(&x);
        self.term_tabs.append(&tab);

        let tv = Rc::new(TermView {
            term: t.clone(),
            view: view.clone(),
            tab,
            tab_btn: tab_btn.clone(),
            tags: RefCell::new(HashMap::new()),
            seen: Cell::new(0),
            back: Cell::new(0),
            dirty: Cell::new(true),
        });
        {
            let (d, id) = (self.clone(), t.id);
            tab_btn.connect_clicked(move |_| d.pick_terminal(id));
        }
        {
            let (d, t) = (self.clone(), t.clone());
            x.connect_clicked(move |_| {
                t.close();
                d.sync_terminals();
            });
        }
        // typing
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let tv = tv.clone();
            keys.connect_key_pressed(move |_, key, _, state| {
                use gtk::gdk::ModifierType as M;
                let ctrl = state.contains(M::CONTROL_MASK);
                let shift = state.contains(M::SHIFT_MASK);
                if ctrl && shift {
                    match key {
                        gtk::gdk::Key::C | gtk::gdk::Key::c => {
                            let b = tv.view.buffer();
                            if let Some((s, e)) = b.selection_bounds() {
                                tv.view.clipboard().set_text(&b.text(&s, &e, false));
                            }
                            return glib::Propagation::Stop;
                        }
                        gtk::gdk::Key::V | gtk::gdk::Key::v => {
                            let t = tv.term.clone();
                            tv.view.clipboard().read_text_async(None::<&gio::Cancellable>, move |r| {
                                if let Ok(Some(s)) = r {
                                    t.write(s.as_bytes());
                                }
                            });
                            return glib::Propagation::Stop;
                        }
                        _ => {}
                    }
                }
                // Ctrl + - 0 belong to the interface size
                if ctrl && matches!(key, gtk::gdk::Key::plus | gtk::gdk::Key::minus | gtk::gdk::Key::equal | gtk::gdk::Key::_0 | gtk::gdk::Key::underscore) {
                    return glib::Propagation::Proceed;
                }
                let app_cursor = tv.term.with_screen(0, |s| s.application_cursor());
                match key_bytes(key, state, app_cursor) {
                    Some(b) => {
                        tv.back.set(0);
                        tv.dirty.set(true);
                        tv.term.write(&b);
                        glib::Propagation::Stop
                    }
                    None => glib::Propagation::Proceed,
                }
            });
        }
        view.add_controller(keys);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let tv = tv.clone();
            scroll.connect_scroll(move |_, _, dy| {
                let b = tv.back.get() as f64 - dy * 3.0;
                tv.back.set(b.clamp(0.0, 2000.0) as usize);
                tv.dirty.set(true);
                glib::Propagation::Stop
            });
        }
        view.add_controller(scroll);
        let click = gtk::GestureClick::new();
        {
            let v = view.clone();
            click.connect_pressed(move |_, _, _, _| {
                v.grab_focus();
            });
        }
        view.add_controller(click);
        tv
    }

    /// A window too narrow for a side panel: the pinned Context then lives in a popup instead.
    pub fn narrow(&self) -> bool {
        let w = self.paned.root().and_downcast::<gtk::Window>().map(|w| w.width()).unwrap_or(0);
        w > 0 && w < crate::theme::zs(760)
    }

    fn tick(self: &Rc<Self>) {
        let only_context = self.tab_buttons.borrow().len() == 1 && self.tab_buttons.borrow().contains_key("context");
        if self.context_pinned.get() && only_context {
            if self.narrow() && self.root.is_visible() {
                self.hide();
                self.auto_hidden.set(true);
            } else if !self.narrow() && self.auto_hidden.get() {
                self.auto_hidden.set(false);
                self.show("context");
            }
        }
        self.sync_terminals();
        if !self.root.is_visible() || self.stack.visible_child_name().as_deref() != Some("terminal") {
            return;
        }
        let Some(name) = self.term_stack.visible_child_name() else { return };
        let Some(id) = name.strip_prefix('t').and_then(|n| n.parse::<u32>().ok()) else { return };
        let Some(tv) = self.term_views.borrow().get(&id).cloned() else { return };
        // the character grid that fits, so programs get the right size
        let layout = tv.view.create_pango_layout(Some("M"));
        let (cw, ch) = layout.pixel_size();
        if cw > 0 && ch > 0 {
            let (w, h) = (tv.view.width() - 14, tv.view.height() - 8);
            if w > cw * 10 && h > ch * 2 {
                tv.term.resize((h / ch) as u16, (w / cw) as u16);
            }
        }
        let v = tv.term.version.load(std::sync::atomic::Ordering::SeqCst);
        if v != tv.seen.get() || tv.dirty.get() {
            tv.seen.set(v);
            tv.dirty.set(false);
            tv.draw();
        }
    }

    // ------------------------------------------------------------ browser

    fn ensure_browser(self: &Rc<Self>) -> Rc<Browser> {
        if let Some(b) = self.browser.borrow().clone() {
            return b;
        }
        let web = webkit6::WebView::new();
        web.set_vexpand(true);
        web.set_hexpand(true);
        let bar = hbox(4);
        bar.add_css_class("browser-bar");
        let back = icon_button("go-previous-symbolic", "Back", "icon-btn");
        let fwd = icon_button("go-next-symbolic", "Forward", "icon-btn");
        let reload = icon_button("view-refresh-symbolic", "Reload", "icon-btn");
        let url = gtk::Entry::new();
        url.set_hexpand(true);
        url.set_placeholder_text(Some("Search or enter an address"));
        for b in [&back, &fwd, &reload] {
            bar.append(b);
        }
        bar.append(&url);
        self.browser_box.append(&bar);
        self.browser_box.append(&web);
        let br = Rc::new(Browser { web: web.clone(), url: url.clone(), pending_load: RefCell::new(None) });
        {
            let w = web.clone();
            back.connect_clicked(move |_| w.go_back());
            let w = web.clone();
            fwd.connect_clicked(move |_| w.go_forward());
            let w = web.clone();
            reload.connect_clicked(move |_| w.reload());
            let w = web.clone();
            url.connect_activate(move |e| w.load_uri(&normalize_url(&e.text())));
        }
        {
            let u = url.clone();
            web.connect_uri_notify(move |w| {
                if let Some(uri) = w.uri() {
                    if !u.has_focus() {
                        u.set_text(&uri);
                    }
                }
            });
            let b = br.clone();
            web.connect_load_changed(move |w, ev| {
                if ev == webkit6::LoadEvent::Finished {
                    if let Some(r) = b.pending_load.borrow_mut().take() {
                        let _ = r.send(Ok(format!("Loaded: {} ({})", w.title().unwrap_or_default(), w.uri().unwrap_or_default())));
                    }
                }
            });
        }
        web.load_uri("https://duckduckgo.com/");
        *self.browser.borrow_mut() = Some(br.clone());
        br
    }

    fn browser_request(self: &Rc<Self>, name: &str, a: &Value, reply: Reply) {
        self.show("browser");
        let br = self.ensure_browser();
        let arg = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match name {
            "zohara_browser_open" => {
                if let Some(old) = br.pending_load.borrow_mut().take() {
                    let _ = old.send(Err("Another page was opened before this one finished loading.".into()));
                }
                *br.pending_load.borrow_mut() = Some(reply);
                br.web.load_uri(&normalize_url(&arg("url")));
                let b = br.clone();
                glib::timeout_add_local_once(Duration::from_secs(25), move || {
                    if let Some(r) = b.pending_load.borrow_mut().take() {
                        let _ = r.send(Ok(format!("Still loading after 25 s: {}", b.web.uri().unwrap_or_default())));
                    }
                });
            }
            "zohara_browser_read" => eval(&br.web, READ_JS, move |r| {
                let _ = reply.send(r.and_then(|s| format_read(&s)));
            }),
            "zohara_browser_click" => {
                let js = click_js(&arg("target"));
                let web = br.web.clone();
                eval(&br.web, &js, move |r| match r {
                    Ok(s) if s.starts_with("ERR") => {
                        let _ = reply.send(Err(s[3..].trim().to_string()));
                    }
                    Ok(s) => {
                        // a click often loads a new page: give it a moment, then report where we are
                        glib::timeout_add_local_once(Duration::from_millis(1800), move || {
                            let _ = reply.send(Ok(format!("{s}\nNow at: {} ({})", web.title().unwrap_or_default(), web.uri().unwrap_or_default())));
                        });
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                });
            }
            "zohara_browser_type" => {
                let js = type_js(&arg("target"), &arg("text"), a.get("submit").and_then(Value::as_bool).unwrap_or(false));
                let web = br.web.clone();
                eval(&br.web, &js, move |r| match r {
                    Ok(s) if s.starts_with("ERR") => {
                        let _ = reply.send(Err(s[3..].trim().to_string()));
                    }
                    Ok(s) => {
                        glib::timeout_add_local_once(Duration::from_millis(1200), move || {
                            let _ = reply.send(Ok(format!("{s}\nNow at: {} ({})", web.title().unwrap_or_default(), web.uri().unwrap_or_default())));
                        });
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                });
            }
            "zohara_browser_scroll" => {
                let js = match arg("where").as_str() {
                    "up" => "window.scrollBy(0, -window.innerHeight*0.8); 'scrolled up'",
                    "top" => "window.scrollTo(0,0); 'at the top'",
                    "bottom" => "window.scrollTo(0, document.body.scrollHeight); 'at the bottom'",
                    _ => "window.scrollBy(0, window.innerHeight*0.8); 'scrolled down'",
                };
                eval(&br.web, js, move |r| {
                    let _ = reply.send(r);
                });
            }
            "zohara_browser_back" => {
                let fwd = a.get("forward").and_then(Value::as_bool).unwrap_or(false);
                if fwd { br.web.go_forward() } else { br.web.go_back() }
                let web = br.web.clone();
                glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                    let _ = reply.send(Ok(format!("Now at: {} ({})", web.title().unwrap_or_default(), web.uri().unwrap_or_default())));
                });
            }
            "zohara_browser_screenshot" => {
                br.web.snapshot(webkit6::SnapshotRegion::Visible, webkit6::SnapshotOptions::NONE, None::<&gio::Cancellable>, move |r| {
                    let out = r.map_err(|e| e.to_string()).and_then(|tex| {
                        let dir = zo_core::audit::data_dir().join("browser");
                        let _ = std::fs::create_dir_all(&dir);
                        let path = dir.join(format!("page-{}.png", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)));
                        tex.save_to_png(&path).map_err(|e| e.to_string())?;
                        Ok(path.to_string_lossy().into_owned())
                    });
                    let _ = reply.send(out);
                });
            }
            "zohara_browser_js" => eval(&br.web, &arg("script"), move |r| {
                let _ = reply.send(r);
            }),
            other => {
                let _ = reply.send(Err(format!("unknown browser tool: {other}")));
            }
        }
    }
}

// ---------------------------------------------------------------- browser helpers

fn normalize_url(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://") || s.starts_with("about:") {
        s.to_string()
    } else if !s.contains(' ') && s.contains('.') {
        format!("https://{s}")
    } else {
        let q: String = s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        format!("https://duckduckgo.com/?q={q}")
    }
}

fn eval(web: &webkit6::WebView, script: &str, done: impl FnOnce(Result<String, String>) + 'static) {
    web.evaluate_javascript(script, None, None, None::<&gio::Cancellable>, move |r| {
        done(match r {
            Ok(v) => {
                if v.is_string() { Ok(v.to_str().to_string()) } else if v.is_undefined() || v.is_null() { Ok(String::new()) } else { Ok(v.to_str().to_string()) }
            }
            Err(e) => Err(e.to_string()),
        })
    });
}

const READ_JS: &str = r#"(() => {
  document.querySelectorAll('[data-zo]').forEach(e => e.removeAttribute('data-zo'));
  let n = 0;
  const vis = e => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };
  const els = [...document.querySelectorAll('a[href],button,input,textarea,select,[role=button],[onclick]')].filter(vis).slice(0, 80);
  const items = els.map(e => {
    e.setAttribute('data-zo', ++n);
    const t = (e.innerText || e.value || e.placeholder || e.getAttribute('aria-label') || e.title || '').trim().replace(/\s+/g, ' ').slice(0, 80);
    return n + ': [' + e.tagName.toLowerCase() + (e.type ? ':' + e.type : '') + '] ' + t + (e.href ? ' -> ' + e.href.slice(0, 100) : '');
  });
  return JSON.stringify({ url: location.href, title: document.title, text: (document.body ? document.body.innerText : '').slice(0, 6000), items });
})()"#;

fn format_read(s: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(s).map_err(|_| "The page could not be read.".to_string())?;
    let items: Vec<String> = v["items"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    Ok(format!(
        "URL: {}\nTitle: {}\n\nPAGE TEXT:\n{}\n\nCLICKABLE (use the number with zohara_browser_click / zohara_browser_type):\n{}",
        v["url"].as_str().unwrap_or(""),
        v["title"].as_str().unwrap_or(""),
        v["text"].as_str().unwrap_or(""),
        items.join("\n")
    ))
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// JavaScript that finds an element by number, selector or visible text.
fn finder(target: &str) -> String {
    let t = target.trim();
    if t.chars().all(|c| c.is_ascii_digit()) && !t.is_empty() {
        format!("document.querySelector('[data-zo=\"{t}\"]')")
    } else if t.starts_with('#') || t.starts_with('.') || t.starts_with('[') || t.contains('>') {
        format!("document.querySelector({})", js_str(t))
    } else {
        format!("[...document.querySelectorAll('a,button,[role=button],input[type=submit],label,summary')].find(e => (e.innerText||e.value||'').toLowerCase().includes({}))", js_str(&t.to_lowercase()))
    }
}

fn click_js(target: &str) -> String {
    format!("(() => {{ const e = {}; if (!e) return 'ERR Nothing matches that. Read the page again to get fresh numbers.'; e.scrollIntoView({{block:'center'}}); e.click(); return 'Clicked ' + (e.innerText||e.value||e.tagName).trim().slice(0,60); }})()", finder(target))
}

fn type_js(target: &str, text: &str, submit: bool) -> String {
    format!(
        "(() => {{ const e = {}; if (!e) return 'ERR Nothing matches that. Read the page again to get fresh numbers.'; e.scrollIntoView({{block:'center'}}); e.focus(); \
         const proto = e.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; \
         const set = Object.getOwnPropertyDescriptor(proto, 'value'); if (set && set.set) set.set.call(e, {}); else e.value = {}; \
         e.dispatchEvent(new Event('input', {{bubbles:true}})); e.dispatchEvent(new Event('change', {{bubbles:true}})); \
         if ({}) {{ const f = e.form; e.dispatchEvent(new KeyboardEvent('keydown', {{key:'Enter', code:'Enter', keyCode:13, bubbles:true}})); if (f) {{ if (f.requestSubmit) f.requestSubmit(); else f.submit(); }} }} \
         return 'Typed into ' + (e.name||e.id||e.tagName) + ({} ? ' and pressed Enter' : ''); }})()",
        finder(target), js_str(text), js_str(text), submit, submit
    )
}

// ---------------------------------------------------------------- terminal drawing

const FG: (u8, u8, u8) = (216, 222, 233);
const BG: (u8, u8, u8) = (15, 17, 21);

fn palette(i: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0x1d, 0x1f, 0x21), (0xcc, 0x66, 0x66), (0xb5, 0xbd, 0x68), (0xf0, 0xc6, 0x74), (0x81, 0xa2, 0xbe), (0xb2, 0x94, 0xbb), (0x8a, 0xbe, 0xb7), (0xc5, 0xc8, 0xc6),
        (0x66, 0x6b, 0x73), (0xd5, 0x4e, 0x53), (0xb9, 0xca, 0x4a), (0xe7, 0xc5, 0x47), (0x7a, 0xa6, 0xda), (0xc3, 0x97, 0xd8), (0x70, 0xc0, 0xb1), (0xff, 0xff, 0xff),
    ];
    match i {
        0..=15 => BASE[i as usize],
        16..=231 => {
            let n = i - 16;
            let c = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (c(n / 36), c((n / 6) % 6), c(n % 6))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            (g, g, g)
        }
    }
}

fn rgb(c: zo_core::term::vt100::Color) -> Option<(u8, u8, u8)> {
    match c {
        zo_core::term::vt100::Color::Default => None,
        zo_core::term::vt100::Color::Idx(i) => Some(palette(i)),
        zo_core::term::vt100::Color::Rgb(r, g, b) => Some((r, g, b)),
    }
}

#[derive(Clone, PartialEq)]
struct Style {
    fg: Option<(u8, u8, u8)>,
    bg: Option<(u8, u8, u8)>,
    bold: bool,
    underline: bool,
}

impl Style {
    fn key(&self) -> String {
        format!("{:?}{:?}{}{}", self.fg, self.bg, self.bold, self.underline)
    }
}

fn rgba(c: (u8, u8, u8)) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::new(c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0, 1.0)
}

impl TermView {
    fn tag(&self, s: &Style) -> gtk::TextTag {
        let key = s.key();
        if let Some(t) = self.tags.borrow().get(&key) {
            return t.clone();
        }
        let b = self.view.buffer();
        let t = gtk::TextTag::new(None);
        if let Some(c) = s.fg {
            t.set_foreground_rgba(Some(&rgba(c)));
        }
        if let Some(c) = s.bg {
            t.set_background_rgba(Some(&rgba(c)));
        }
        if s.bold {
            t.set_weight(700);
        }
        if s.underline {
            t.set_underline(gtk::pango::Underline::Single);
        }
        b.tag_table().add(&t);
        self.tags.borrow_mut().insert(key, t.clone());
        t
    }

    fn draw(&self) {
        let back = self.back.get();
        let runs: Vec<(String, Style)> = self.term.with_screen(back, |s| {
            let (rows, cols) = s.size();
            let cursor = (!s.hide_cursor() && back == 0).then(|| s.cursor_position());
            let mut out: Vec<(String, Style)> = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    let (text, style) = match s.cell(r, c) {
                        Some(cell) => {
                            if cell.is_wide_continuation() {
                                continue;
                            }
                            let t = cell.contents();
                            let (mut fg, mut bg) = (rgb(cell.fgcolor()), rgb(cell.bgcolor()));
                            if cell.inverse() {
                                let (f, b) = (fg.unwrap_or(FG), bg.unwrap_or(BG));
                                fg = Some(b);
                                bg = Some(f);
                            }
                            (if t.is_empty() { " ".to_string() } else { t }, Style { fg, bg, bold: cell.bold(), underline: cell.underline() })
                        }
                        None => (" ".to_string(), Style { fg: None, bg: None, bold: false, underline: false }),
                    };
                    let style = if cursor == Some((r, c)) { Style { fg: Some(BG), bg: Some(FG), ..style } } else { style };
                    match out.last_mut() {
                        Some((t, st)) if *st == style => t.push_str(&text),
                        _ => out.push((text, style)),
                    }
                }
                out.push(("\n".to_string(), Style { fg: None, bg: None, bold: false, underline: false }));
            }
            out
        });
        let b = self.view.buffer();
        b.set_text("");
        for (text, style) in runs {
            let mut end = b.end_iter();
            if style.fg.is_none() && style.bg.is_none() && !style.bold && !style.underline {
                b.insert(&mut end, &text);
            } else {
                b.insert_with_tags(&mut end, &text, &[&self.tag(&style)]);
            }
        }
    }
}

// ---------------------------------------------------------------- keys

fn key_bytes(key: gtk::gdk::Key, state: gtk::gdk::ModifierType, app_cursor: bool) -> Option<Vec<u8>> {
    use gtk::gdk::{Key, ModifierType as M};
    let ctrl = state.contains(M::CONTROL_MASK);
    let alt = state.contains(M::ALT_MASK);
    let arrow = |c: u8| if app_cursor { vec![0x1b, b'O', c] } else { vec![0x1b, b'[', c] };
    let named: Option<Vec<u8>> = match key {
        Key::Return | Key::KP_Enter => Some(b"\r".to_vec()),
        Key::BackSpace => Some(vec![0x7f]),
        Key::Tab => Some(b"\t".to_vec()),
        Key::ISO_Left_Tab => Some(b"\x1b[Z".to_vec()),
        Key::Escape => Some(vec![0x1b]),
        Key::Up => Some(arrow(b'A')),
        Key::Down => Some(arrow(b'B')),
        Key::Right => Some(arrow(b'C')),
        Key::Left => Some(arrow(b'D')),
        Key::Home => Some(b"\x1b[H".to_vec()),
        Key::End => Some(b"\x1b[F".to_vec()),
        Key::Page_Up => Some(b"\x1b[5~".to_vec()),
        Key::Page_Down => Some(b"\x1b[6~".to_vec()),
        Key::Delete => Some(b"\x1b[3~".to_vec()),
        Key::Insert => Some(b"\x1b[2~".to_vec()),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let ch = key.to_unicode()?;
    if ctrl && ch.is_ascii_alphabetic() {
        return Some(vec![(ch.to_ascii_lowercase() as u8) & 0x1f]);
    }
    if ctrl {
        return match ch {
            ' ' | '@' => Some(vec![0]),
            '[' => Some(vec![0x1b]),
            '\\' => Some(vec![0x1c]),
            ']' => Some(vec![0x1d]),
            _ => None,
        };
    }
    if ch.is_control() {
        return None;
    }
    let mut b = ch.to_string().into_bytes();
    if alt {
        b.insert(0, 0x1b);
    }
    Some(b)
}
