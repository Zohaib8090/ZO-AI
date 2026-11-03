//! The chat: sending a message to the chosen model, showing what it says and the tools it runs,
//! and the sheet where the person picks a provider, a model and an API key.

use crate::chats::{self, Chat, Entry};
use crate::composer::Prompt;
use crate::{hbox, lbl, primary, secondary, sheet, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use zo_core::ai::agent::{self, Config, Event};
use zo_core::ai::claude_code;
use zo_core::ai::wire::Turn;
use zo_core::ai::providers::Protocol;
use zo_core::ai::{keys, providers, Selection};

pub struct Ctl {
    chat: gtk::Box,
    hero: gtk::Box,
    thread: gtk::Box,
    scroll: gtk::ScrolledWindow,
    history: RefCell<Vec<Turn>>,
    /// Claude Code mode keeps its own conversation; this is its session id.
    session: RefCell<Option<String>>,
    /// What the model should know about the project this chat belongs to.
    extra: RefCell<String>,
    title: RefCell<String>,
    id: RefCell<String>,
    /// Everything shown in this chat, so it can be saved and shown again.
    entries: RefCell<Vec<Entry>>,
    on_saved: RefCell<Option<Rc<dyn Fn()>>>,
    busy: Cell<bool>,
    cancel: Arc<AtomicBool>,
    on_busy: RefCell<Option<Rc<dyn Fn(bool)>>>,
    on_retry: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    last_retry: RefCell<Option<gtk::Button>>,
}

enum Msg {
    Ev(Event),
    Done(Result<(), String>, Vec<Turn>, Option<String>),
}

impl Ctl {
    /// Adds the message list between the greeting and the composer.
    pub fn new(chat: &gtk::Box, hero: &gtk::Box) -> Rc<Ctl> {
        let thread = vbox(10);
        thread.set_halign(gtk::Align::Center);
        thread.set_size_request(760, -1);
        thread.add_css_class("thread");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&thread));
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_visible(false);
        chat.insert_child_after(&scroll, Some(hero));
        Rc::new(Ctl { chat: chat.clone(), hero: hero.clone(), thread, scroll, history: RefCell::new(Vec::new()), session: RefCell::new(None), extra: RefCell::new(String::new()), title: RefCell::new(String::new()), id: RefCell::new(String::new()), entries: RefCell::new(Vec::new()), on_saved: RefCell::new(None), busy: Cell::new(false), cancel: Arc::new(AtomicBool::new(false)), on_busy: RefCell::new(None), on_retry: Rc::new(RefCell::new(None)), last_retry: RefCell::new(None) })
    }

    pub fn on_saved(&self, f: Rc<dyn Fn()>) {
        *self.on_saved.borrow_mut() = Some(f);
    }

    fn persist(&self) {
        let entries = self.entries.borrow().clone();
        if entries.is_empty() {
            return;
        }
        chats::upsert(Chat {
            id: self.id.borrow().clone(),
            title: self.title.borrow().clone(),
            updated: chats::now_millis(),
            entries,
            history: self.history.borrow().clone(),
            session: self.session.borrow().clone(),
            context: self.extra.borrow().clone(),
            pinned: false,
        });
        let f = self.on_saved.borrow().clone();
        if let Some(f) = f {
            f();
        }
    }

    /// Opens a saved chat again, ready to continue.
    pub fn load_chat(&self, chat: Chat) {
        if self.busy.get() {
            return;
        }
        self.new_conversation();
        *self.id.borrow_mut() = chat.id;
        *self.title.borrow_mut() = chat.title;
        *self.history.borrow_mut() = chat.history;
        *self.session.borrow_mut() = chat.session;
        *self.extra.borrow_mut() = chat.context;
        self.hero.set_visible(false);
        self.scroll.set_visible(true);
        self.chat.set_valign(gtk::Align::Fill);
        for e in chat.entries {
            self.add(&e.text, &e.kind);
        }
    }

    /// Called when "try again" is pressed.
    pub fn on_retry(&self, f: Rc<dyn Fn()>) {
        *self.on_retry.borrow_mut() = Some(f);
    }

    /// Throws away the last reply and asks the same thing again.
    pub fn retry(self: &Rc<Self>, prompt: &Prompt) {
        if self.busy.get() {
            return;
        }
        let entries = self.entries.borrow().clone();
        let Some(at) = entries.iter().rposition(|e| e.kind == "msg-user") else { return };
        let text = entries[at].text.clone();
        let last_user = self.history.borrow().iter().rposition(|t| matches!(t, Turn::User(_)));
        if let Some(p) = last_user {
            self.history.borrow_mut().truncate(p);
        }
        // show the chat again without the last question and its answer
        while let Some(c) = self.thread.first_child() {
            self.thread.remove(&c);
        }
        *self.last_retry.borrow_mut() = None;
        self.entries.borrow_mut().clear();
        for e in &entries[..at] {
            self.add(&e.text, &e.kind);
        }
        prompt.set_text(&text);
        self.submit(prompt);
    }

    pub fn is_busy(&self) -> bool {
        self.busy.get()
    }

    /// Ends what is running now (the model call between steps, or the Claude Code program).
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn on_busy(&self, f: Rc<dyn Fn(bool)>) {
        *self.on_busy.borrow_mut() = Some(f);
    }

    fn set_busy(&self, on: bool) {
        self.busy.set(on);
        let f = self.on_busy.borrow().clone();
        if let Some(f) = f {
            f(on);
        }
    }

    pub fn set_context(&self, text: String) {
        *self.extra.borrow_mut() = text;
    }

    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    pub fn new_conversation(&self) {
        if self.busy.get() {
            return;
        }
        self.history.borrow_mut().clear();
        *self.session.borrow_mut() = None;
        self.extra.borrow_mut().clear();
        self.title.borrow_mut().clear();
        self.id.borrow_mut().clear();
        self.entries.borrow_mut().clear();
        while let Some(c) = self.thread.first_child() {
            self.thread.remove(&c);
        }
        self.scroll.set_visible(false);
        self.hero.set_visible(true);
        self.chat.set_valign(gtk::Align::Center);
    }

    /// Shows a line and remembers it for saving.
    fn add(&self, text: &str, class: &str) -> gtk::Widget {
        self.entries.borrow_mut().push(Entry { kind: class.to_string(), text: text.to_string() });
        self.add_plain(text, class)
    }

    fn scroll_down(&self) {
        let scroll = self.scroll.clone();
        glib::timeout_add_local_once(Duration::from_millis(40), move || {
            let a = scroll.vadjustment();
            a.set_value(a.upper() - a.page_size());
        });
    }

    fn add_plain(&self, text: &str, class: &str) -> gtk::Widget {
        let w: gtk::Widget = if class == "msg-ai" {
            // a reply: formatted, with a copy button under it
            let col = vbox(2);
            col.append(&crate::markdown::render(text));
            let actions = hbox(2);
            actions.add_css_class("msg-actions");
            let copy = crate::icon_button("edit-copy-symbolic", "Copy the reply", "icon-btn");
            let raw = text.to_string();
            copy.connect_clicked(move |b| b.clipboard().set_text(&raw));
            actions.append(&copy);
            // "try again" sits under the newest reply only
            let retry = crate::icon_button("view-refresh-symbolic", "Try again", "icon-btn");
            let on_retry = self.on_retry.clone();
            retry.connect_clicked(move |_| {
                let f = on_retry.borrow().clone();
                if let Some(f) = f {
                    f();
                }
            });
            actions.append(&retry);
            if let Some(old) = self.last_retry.borrow_mut().replace(retry) {
                old.set_visible(false);
            }
            col.append(&actions);
            col.upcast()
        } else {
            let l = lbl(text, class);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            l.set_xalign(0.0);
            l.set_selectable(true);
            l.set_halign(if class == "msg-user" { gtk::Align::End } else { gtk::Align::Fill });
            l.upcast()
        };
        let l = w;
        self.thread.append(&l);
        let scroll = self.scroll.clone();
        glib::timeout_add_local_once(Duration::from_millis(40), move || {
            let a = scroll.vadjustment();
            a.set_value(a.upper() - a.page_size());
        });
        l
    }

    pub fn submit(self: &Rc<Self>, prompt: &Prompt) {
        let text = prompt.text().trim().to_string();
        if text.is_empty() || self.busy.get() {
            return;
        }
        let sel = Selection::load();
        self.hero.set_visible(false);
        self.scroll.set_visible(true);
        self.chat.set_valign(gtk::Align::Fill);
        prompt.set_text("");
        self.add(&text, "msg-user");
        if self.title.borrow().is_empty() {
            let t: String = text.chars().take(48).collect();
            *self.title.borrow_mut() = t;
        }
        let system = format!("{}\n\n{}", agent::SYSTEM_PROMPT, self.extra.borrow());
        let extra = self.extra.borrow().clone();

        let claude = sel.provider == "claude-code";
        let cfg = if claude {
            None
        } else {
            match Config::from_selection(&sel, &system) {
                Ok(c) => Some(c),
                Err(e) => {
                    self.add(&e, "msg-error");
                    return;
                }
            }
        };
        let model = if sel.model.is_empty() { "sonnet".to_string() } else { sel.model.clone() };
        if self.id.borrow().is_empty() {
            *self.id.borrow_mut() = format!("c{}", chats::now_millis());
        }
        self.cancel.store(false, Ordering::Relaxed);
        self.set_busy(true);
        let thinking = self.add_plain("Thinking…", "msg-tool");
        // the reply as it is being written, shown plain until it is complete
        let mut live: Option<(gtk::Widget, String)> = None;
        let (tx, rx) = mpsc::channel::<Msg>();
        let mut history = std::mem::take(&mut *self.history.borrow_mut());
        let mut session = self.session.borrow().clone();
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let mut emit = |e: Event| {
                let _ = tx2.send(Msg::Ev(e));
            };
            let r = match &cfg {
                None => claude_code::run(&model, &mut session, &text, &extra, &cancel, &mut emit),
                Some(cfg) => agent::run(cfg, &mut history, &text, zo_core::Mode::load(), &zo_core::approval::AppApprover, &cancel, &mut emit),
            };
            let _ = tx.send(Msg::Done(r, history, session));
        });
        let me = self.clone();
        let prompt = prompt.clone();
        glib::timeout_add_local(Duration::from_millis(40), move || {
            loop {
                match rx.try_recv() {
                    Ok(Msg::Ev(Event::TextDelta(d))) => {
                        thinking.set_visible(false);
                        let entry = live.get_or_insert_with(|| (me.add_plain("", "msg-live"), String::new()));
                        entry.1.push_str(&d);
                        if let Some(l) = entry.0.downcast_ref::<gtk::Label>() {
                            l.set_text(&entry.1);
                        }
                        me.scroll_down();
                    }
                    Ok(Msg::Ev(Event::Text(t))) => {
                        thinking.set_visible(false);
                        if let Some((w, _)) = live.take() {
                            me.thread.remove(&w);
                        }
                        me.add(&t, "msg-ai");
                        // code in the reply is kept under Artifacts
                        let chat_id = me.id.borrow().clone();
                        for b in crate::markdown::parse(&t) {
                            if let crate::markdown::Block::Code { lang, text } = b {
                                zo_core::store::artifacts::add_code(&lang, &text, Some(chat_id.clone()));
                            }
                        }
                    }
                    Ok(Msg::Ev(Event::ToolStart { name, summary })) => {
                        thinking.set_visible(false);
                        let detail = summary.lines().nth(1).unwrap_or("");
                        me.add(&format!("▸ {name}  {detail}"), "msg-tool");
                    }
                    Ok(Msg::Ev(Event::ToolDone { ok, output, .. })) => {
                        let short: String = output.chars().take(300).collect();
                        me.add(&format!("{} {}", if ok { "✓" } else { "✗" }, short), if ok { "msg-tool-out" } else { "msg-error" });
                    }
                    Ok(Msg::Done(result, history, session)) => {
                        thinking.set_visible(false);
                        // stopped half way: keep what had been written
                        if let Some((w, partial)) = live.take() {
                            me.thread.remove(&w);
                            if !partial.is_empty() {
                                me.add(&partial, "msg-ai");
                            }
                        }
                        *me.history.borrow_mut() = history;
                        *me.session.borrow_mut() = session;
                        match result {
                            Err(e) if e == agent::STOPPED => {
                                me.add(&e, "msg-tool");
                            }
                            Err(e) => {
                                me.add(&e, "msg-error");
                            }
                            Ok(()) => {}
                        }
                        me.persist();
                        me.set_busy(false);
                        prompt.set_sensitive(true);
                        prompt.grab_focus();
                        return glib::ControlFlow::Break;
                    }
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        me.set_busy(false);
                        prompt.set_sensitive(true);
                        return glib::ControlFlow::Break;
                    }
                }
            }
        });
    }
}

/// A friendlier name for the model ids people know.
fn nice_name(model: &str) -> String {
    match model {
        "claude-fable-5-1" => "Fable 5.1",
        "claude-opus-5-5" => "Opus 5.5",
        "claude-sonnet-5-5" => "Sonnet 5.5",
        "claude-haiku-5-5" => "Haiku 5.5",
        "sonnet" => "Sonnet",
        "opus" => "Opus",
        "haiku" => "Haiku",
        other => other,
    }
    .to_string()
}

/// What the model button says.
pub fn pill_text(sel: &Selection) -> String {
    match providers::find(&sel.provider) {
        Some(_) if !sel.model.is_empty() => nice_name(&sel.model),
        Some(p) if p.protocol == Protocol::ClaudeCode => "Sonnet".to_string(),
        _ => "Choose a model".to_string(),
    }
}

pub fn models_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = sheet(ui, "Model and API key", 520);
    let sel = Selection::load();
    let all = providers::all();

    let names: Vec<&str> = all.iter().map(|p| p.name).collect();
    let provider_dd = gtk::DropDown::from_strings(&names);
    provider_dd.set_selected(all.iter().position(|p| p.id == sel.provider).unwrap_or(0) as u32);
    body.append(&crate::field("Provider", &provider_dd));

    let key = gtk::PasswordEntry::new();
    key.set_show_peek_icon(true);
    key.add_css_class("field-input");
    let key_field = crate::field("API key", &key);
    body.append(&key_field);

    let url = gtk::Entry::new();
    url.add_css_class("field-input");
    let url_field = crate::field("Address", &url);
    body.append(&url_field);

    let model = gtk::Entry::new();
    model.add_css_class("field-input");
    model.set_placeholder_text(Some("Model name"));
    let load = secondary("Load the list");
    let pick = gtk::DropDown::from_strings(&[]);
    pick.set_visible(false);
    let model_row = hbox(8);
    model.set_hexpand(true);
    model_row.append(&model);
    model_row.append(&load);
    let model_col = vbox(7);
    model_col.append(&model_row);
    model_col.append(&pick);
    body.append(&crate::field("Model", &model_col));

    let status = lbl("", "field-label");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);

    let current = move |dd: &gtk::DropDown| &providers::all()[dd.selected() as usize];

    // fill the fields for the provider that is selected
    let refresh = {
        let (key, key_field, url_field, url, model, status, sel) = (key.clone(), key_field.clone(), url_field.clone(), url.clone(), model.clone(), status.clone(), Rc::new(RefCell::new(sel.clone())));
        move |dd: &gtk::DropDown| {
            let p = current(dd);
            let s = sel.borrow();
            url.set_text(&s.base_url_for(p));
            key_field.set_visible(!p.local);
            url_field.set_visible(p.protocol != Protocol::ClaudeCode);
            key.set_text("");
            key.set_placeholder_text(Some(if keys::get(p.id).is_some() { "Saved in the keyring — type to replace" } else { "Paste the key" }));
            model.set_text(if s.provider == p.id { &s.model } else { p.models.first().copied().unwrap_or("") });
            status.set_text(if p.protocol == Protocol::ClaudeCode {
                if claude_code::binary().is_some() { "Uses the Claude Code program you installed and signed in to yourself (claude auth login). ZO-AI never sees your login. Experimental." } else { "Claude Code is not installed. Install it with: npm install -g @anthropic-ai/claude-code, then sign in with: claude auth login" }
            } else if p.local { "Runs on this computer. No key, and nothing leaves it." } else if keys::available() { "The key is stored in the system keyring." } else { "The keyring tool (secret-tool) is missing, so keys cannot be saved." });
        }
    };
    refresh(&provider_dd);
    provider_dd.connect_selected_notify(move |dd| refresh(dd));

    // show which local servers are already running
    {
        let status = status.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(agent::detect_local().iter().map(|p| p.name.to_string()).collect::<Vec<_>>());
        });
        glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
            Ok(found) => {
                if !found.is_empty() {
                    let t = status.text();
                    status.set_text(&format!("{t}\nFound running here: {}", found.join(", ")));
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        });
    }

    // load the provider's own model list
    {
        let (provider_dd, url, key, status, pick, model2) = (provider_dd.clone(), url.clone(), key.clone(), status.clone(), pick.clone(), model.clone());
        load.connect_clicked(move |b| {
            let p = current(&provider_dd);
            let typed = key.text().to_string();
            let k = if typed.is_empty() { keys::get(p.id) } else { Some(typed) };
            let base = url.text().to_string();
            status.set_text("Loading…");
            b.set_sensitive(false);
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(agent::list_models(p, &base, k.as_deref()));
            });
            let (status, pick, model, b) = (status.clone(), pick.clone(), model2.clone(), b.clone());
            glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
                Ok(r) => {
                    b.set_sensitive(true);
                    match r {
                        Ok(list) if !list.is_empty() => {
                            let strs: Vec<&str> = list.iter().map(String::as_str).collect();
                            pick.set_model(Some(&gtk::StringList::new(&strs)));
                            pick.set_visible(true);
                            if !list.iter().any(|m| *m == model.text()) {
                                model.set_text(&list[0]);
                            }
                            status.set_text(&format!("{} models. Pick one below.", list.len()));
                            let (model, list) = (model.clone(), list.clone());
                            pick.connect_selected_notify(move |d| {
                                if let Some(m) = list.get(d.selected() as usize) {
                                    model.set_text(m);
                                }
                            });
                        }
                        Ok(_) => status.set_text("The provider returned no models."),
                        Err(e) => status.set_text(&e),
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => glib::ControlFlow::Break,
            });
        });
    }

    let cancel = secondary("Cancel");
    let save = primary("Save");
    foot.append(&cancel);
    foot.append(&save);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    {
        let (w, ui2, provider_dd, key, url, model, status) = (win.clone(), ui.clone(), provider_dd.clone(), key.clone(), url.clone(), model.clone(), status.clone());
        save.connect_clicked(move |_| {
            let mut sel = Selection::load();
            let p = current(&provider_dd);
            let typed = key.text().to_string();
            if !typed.trim().is_empty() {
                if let Err(e) = keys::set(p.id, &typed) {
                    status.set_text(&e);
                    return;
                }
            }
            sel.provider = p.id.to_string();
            sel.model = model.text().trim().to_string();
            let u = url.text().trim().to_string();
            if u == p.base_url || u.is_empty() {
                sel.base_urls.remove(p.id);
            } else {
                sel.base_urls.insert(p.id.to_string(), u);
            }
            sel.save();
            ui2.model_label.set_text(&pill_text(&sel));
            ui2.pv_model_label.set_text(&pill_text(&sel));
            w.close();
        });
    }
    win.present();
}


// ---------------------------------------------------------------- the home page pieces

/// "Good evening, Zohaib": the greeting on the home page.
pub fn greeting() -> String {
    let hour = glib::DateTime::now_local().map(|d| d.hour()).unwrap_or(12);
    let part = match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    let full = glib::real_name().to_string_lossy().into_owned();
    let mut name = full.split_whitespace().next().unwrap_or("").to_string();
    if name.is_empty() || name == "Unknown" {
        name = glib::user_name().to_string_lossy().into_owned();
        if let Some(c) = name.get(..1) {
            name = format!("{}{}", c.to_uppercase(), &name[1..]);
        }
    }
    if name.is_empty() {
        part.to_string()
    } else {
        format!("{part}, {name}")
    }
}

/// A flat text button for the line under the composer: the model, or the permission mode.
pub fn pill_button(text: &str, chevron: bool) -> (gtk::Button, gtk::Label) {
    let label = lbl(text, "");
    let b = gtk::Button::new();
    b.add_css_class("tool-btn");
    b.add_css_class("pill");
    let row = hbox(6);
    row.append(&label);
    if chevron {
        row.append(&crate::icon("pan-down-symbolic", 12));
    }
    b.set_child(Some(&row));
    (b, label)
}

fn new_popover(parent: &gtk::Button) -> gtk::Popover {
    let pop = gtk::Popover::new();
    pop.add_css_class("zo-popover");
    pop.set_position(gtk::PositionType::Top);
    pop.set_parent(parent);
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop
}

/// A row in a popover: a title, a small line under it, and a tick when it is the current choice.
fn choice_row(title: &str, sub: Option<&str>, checked: bool) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_has_frame(false);
    b.add_css_class("pop-row");
    let row = hbox(18);
    let col = vbox(2);
    col.set_hexpand(true);
    col.append(&lbl(title, "pop-title"));
    if let Some(s) = sub {
        let l = lbl(s, "pop-sub");
        l.set_wrap(true);
        l.set_max_width_chars(34);
        col.append(&l);
    }
    row.append(&col);
    if checked {
        let tick = crate::icon("object-select-symbolic", 16);
        tick.add_css_class("pop-check");
        row.append(&tick);
    }
    b.set_child(Some(&row));
    b
}

fn separator() -> gtk::Box {
    let sep = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sep.add_css_class("pop-sep");
    sep
}

fn model_choices(p: &providers::Provider, sel: &Selection) -> Vec<(String, String, Option<String>)> {
    let claude = |id: &str, title: &str, desc: &str| (id.to_string(), title.to_string(), Some(desc.to_string()));
    match p.id {
        "anthropic" => vec![
            claude("claude-fable-5-1", "Fable 5.1", "For your toughest challenges"),
            claude("claude-opus-5-5", "Opus 5.5", "For complex work and everyday tasks"),
            claude("claude-sonnet-5-5", "Sonnet 5.5", "Most efficient for simpler tasks"),
            claude("claude-haiku-5-5", "Haiku 5.5", "Fastest for quick answers"),
        ],
        "claude-code" => vec![
            claude("sonnet", "Sonnet", "Most efficient for simpler tasks"),
            claude("opus", "Opus", "For complex work and everyday tasks"),
            claude("haiku", "Haiku", "Fastest for quick answers"),
        ],
        _ => {
            let mut v: Vec<(String, String, Option<String>)> = p.models.iter().map(|m| (m.to_string(), m.to_string(), None)).collect();
            if !sel.model.is_empty() && sel.provider == p.id && !v.iter().any(|(id, _, _)| *id == sel.model) {
                v.insert(0, (sel.model.clone(), sel.model.clone(), None));
            }
            v
        }
    }
}

/// The model menu above the model button.
pub fn show_model_popover(ui: &Rc<Ui>, btn: &gtk::Button) {
    let sel = Selection::load();
    let pop = new_popover(btn);
    let body = vbox(0);
    match providers::find(&sel.provider) {
        Some(p) => {
            body.append(&lbl(p.name, "pop-head"));
            for (id, title, desc) in model_choices(p, &sel) {
                let row = choice_row(&title, desc.as_deref(), sel.provider == p.id && sel.model == id);
                let (ui2, pop2, provider_id) = (ui.clone(), pop.clone(), p.id.to_string());
                row.connect_clicked(move |_| {
                    let mut s = Selection::load();
                    s.provider = provider_id.clone();
                    s.model = id.clone();
                    s.save();
                    ui2.model_label.set_text(&pill_text(&s));
                    ui2.pv_model_label.set_text(&pill_text(&s));
                    pop2.popdown();
                });
                body.append(&row);
            }
            body.append(&separator());
        }
        None => body.append(&lbl("Choose a provider to begin", "pop-head")),
    }
    let more = choice_row("Provider, key and address…", None, false);
    let (ui2, pop2) = (ui.clone(), pop.clone());
    more.connect_clicked(move |_| {
        pop2.popdown();
        models_sheet(&ui2);
    });
    body.append(&more);
    pop.set_child(Some(&body));
    pop.popup();
}

/// The permission menu above the mode button.
pub fn show_perm_popover(ui: &Rc<Ui>, btn: &gtk::Button) {
    let current = zo_core::Mode::load().name();
    let pop = new_popover(btn);
    let body = vbox(0);
    body.append(&lbl("Permission mode", "pop-head"));
    let modes = [
        ("Manual", "ZO-AI asks before every change and every command."),
        ("Auto", "Looking around runs on its own; changes and other commands ask first."),
        ("Bypass", "Nothing asks. Use with care."),
    ];
    for (name, desc) in modes {
        let row = choice_row(name, Some(desc), name == current);
        let (ui2, pop2) = (ui.clone(), pop.clone());
        row.connect_clicked(move |_| {
            zo_core::Mode::parse(name).save();
            ui2.perm_label.set_text(name);
            ui2.pv_perm_label.set_text(name);
            pop2.popdown();
        });
        body.append(&row);
    }
    pop.set_child(Some(&body));
    pop.popup();
}

/// The "+" menu: put a file or a folder path into the message, so the model can look at it.
pub fn show_plus_popover(prompt: &Prompt, btn: &gtk::Button) {
    let pop = new_popover(btn);
    let body = vbox(0);
    for (title, sub, folder) in [("Add a file", "Puts its path in your message", false), ("Add a folder", "Puts its path in your message", true)] {
        let row = choice_row(title, Some(sub), false);
        let (prompt2, pop2) = (prompt.clone(), pop.clone());
        row.connect_clicked(move |_| {
            pop2.popdown();
            let dialog = gtk::FileDialog::new();
            let window = prompt2.window();
            let prompt3 = prompt2.clone();
            let done = move |path: Option<std::path::PathBuf>| {
                if let Some(p) = path {
                    let old = prompt3.text();
                    let sep = if old.is_empty() || old.ends_with(' ') { "" } else { " " };
                    prompt3.set_text(&format!("{old}{sep}{} ", p.display()));
                    prompt3.move_to_end();
                    prompt3.grab_focus();
                }
            };
            if folder {
                dialog.select_folder(window.as_ref(), gtk::gio::Cancellable::NONE, move |r| done(r.ok().and_then(|f| f.path())));
            } else {
                dialog.open(window.as_ref(), gtk::gio::Cancellable::NONE, move |r| done(r.ok().and_then(|f| f.path())));
            }
        });
        body.append(&row);
    }
    pop.set_child(Some(&body));
    pop.popup();
}
