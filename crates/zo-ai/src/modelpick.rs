//! The model chooser ("Model and API key"): provider, key, address, a searchable list of the provider's models with a star on each to
//! pin it to the model menu, and the custom-endpoint flow. Nothing here waits for the network on the window's own thread: the
//! lists are fetched in a worker thread and kept for a minute per provider and address.

use crate::{chat, hbox, lbl, primary, secondary, sheet, vbox, Ui};
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zo_core::ai::providers::{Protocol, Provider};
use zo_core::ai::{agent, keys, profiles, providers, Selection};
use zo_core::ai::ui_prefs as uiprefs;

// ---------------------------------------------------------------- the model lists, fetched off the window thread

static CACHE: Mutex<Option<HashMap<String, (Instant, Vec<String>)>>> = Mutex::new(None);
const CACHE_FOR: Duration = Duration::from_secs(60);

fn cache_key(p: &Provider, base: &str) -> String {
    format!("{}|{}", p.id, base.trim().trim_end_matches('/'))
}

fn cached(p: &Provider, base: &str) -> Option<Vec<String>> {
    let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref()?.get(&cache_key(p, base)).filter(|(t, _)| t.elapsed() < CACHE_FOR).map(|(_, v)| v.clone())
}

fn remember(p: &Provider, base: &str, list: &[String]) {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new).insert(cache_key(p, base), (Instant::now(), list.to_vec()));
}

/// What went wrong, in words a person can act on.
pub(crate) fn friendly_error(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("answered 401") || l.contains("answered 403") {
        "The provider did not accept the key (401). Check it and try again.".into()
    } else if l.contains("answered 404") {
        "Nothing is listed at that address (404). It usually ends in /v1.".into()
    } else if l.contains("answered 429") {
        "The provider says too many requests (429). Wait a moment and retry.".into()
    } else if l.contains("timed out") || l.contains("timeout") {
        "The address did not answer in time. Check it, and that the server is running.".into()
    } else if l.contains("could not reach") {
        "Could not reach that address. Check it, and that the server is running.".into()
    } else if l.contains("not valid json") || l.contains("expected") && l.contains("json") {
        "That address answered, but not like a model server (the reply was not a model list).".into()
    } else {
        e.to_string()
    }
}

/// Fetches the model list in a worker thread and hands the result to `done` on the window's thread. A list fetched in the last
/// minute for the same provider and address is used as it is, unless `force`.
pub(crate) fn fetch_models(p: &'static Provider, base: String, typed_key: Option<String>, force: bool, done: impl FnOnce(Result<Vec<String>, String>) + 'static) {
    if !force {
        if let Some(list) = cached(p, &base) {
            glib::idle_add_local_once(move || done(Ok(list)));
            return;
        }
    }
    glib::spawn_future_local(async move {
        let b2 = base.clone();
        let r = gtk::gio::spawn_blocking(move || {
            let key = typed_key.filter(|k| !k.trim().is_empty()).or_else(|| profiles::get_key(&Selection::load(), p.id));
            if !p.local && p.id != "custom" && p.protocol != Protocol::ClaudeCode && key.is_none() {
                return Err("No API key yet. Paste it above, then press Retry.".to_string());
            }
            let b = if base.trim().is_empty() { p.base_url.to_string() } else { base };
            agent::list_models(p, &b, key.as_deref()).map_err(|e| friendly_error(&e))
        })
        .await
        .unwrap_or_else(|_| Err("The check stopped unexpectedly.".to_string()));
        if let Ok(list) = &r {
            remember(p, &b2, list);
        }
        done(r);
    });
}

/// "Test connection": the same list call, reported as one friendly line.
pub(crate) fn test_connection(p: &'static Provider, base: String, typed_key: Option<String>, done: impl FnOnce(Result<String, String>) + 'static) {
    fetch_models(p, base, typed_key, true, move |r| {
        done(r.map(|l| if l.is_empty() { "Connected, but it listed no models.".to_string() } else { format!("Connected. {} models available.", l.len()) }));
    });
}

/// The providers shown in the pickers: the ones that are not hidden, and always the one in use.
pub(crate) fn visible_providers(keep: &str) -> Vec<&'static Provider> {
    let hidden = uiprefs::hidden_providers();
    providers::all().iter().filter(|p| p.id == keep || !hidden.iter().any(|h| h == p.id)).collect()
}

/// A provider where the person types the address: custom endpoints and local servers.
fn needs_url(p: &Provider) -> bool {
    p.id == "custom" || (p.local && p.protocol != Protocol::ClaudeCode)
}

/// The pinned models of the active profile, when it belongs to this provider.
fn pins_of(sel: &Selection, p: &Provider) -> Vec<String> {
    sel.profile.as_deref().and_then(|id| profiles::load().into_iter().find(|x| x.id == id)).filter(|x| x.provider == p.id).map(|x| x.pinned).unwrap_or_default()
}

// ---------------------------------------------------------------- the model list widget

/// A searchable list of models with a star on each. Pinned ones come first, under their own header.
#[derive(Clone)]
struct ModelList {
    root: gtk::Box,
    list: gtk::ListBox,
    search: gtk::SearchEntry,
    banner: gtk::Box,
    spinner: gtk::Spinner,
    message: gtk::Label,
    retry: gtk::Button,
    count: gtk::Label,
    pinned: Rc<RefCell<Vec<String>>>,
    order: Rc<RefCell<HashMap<String, usize>>>,
    /// Lets an answer that arrives late (for a provider that is no longer shown) be dropped.
    generation: Rc<Cell<u32>>,
    /// The fill that is adding rows now (a newer fill stops an older one).
    fill_gen: Rc<Cell<u32>>,
    current: Rc<RefCell<String>>,
    on_pick: Rc<RefCell<Option<Rc<dyn Fn(&str)>>>>,
}

impl ModelList {
    fn new() -> ModelList {
        let root = vbox(8);
        let head = hbox(8);
        let title = lbl("Models", "field-label");
        title.set_hexpand(true);
        let count = lbl("", "field-hint");
        head.append(&title);
        head.append(&count);
        let reload = crate::icon_button("view-refresh-symbolic", "Load the list again", "icon-btn");
        head.append(&reload);
        root.append(&head);

        let banner = hbox(8);
        let spinner = gtk::Spinner::new();
        let message = lbl("", "field-hint");
        message.set_wrap(true);
        message.set_hexpand(true);
        let retry = secondary("Retry");
        banner.append(&spinner);
        banner.append(&message);
        banner.append(&retry);
        banner.set_visible(false);
        root.append(&banner);

        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search models"));
        root.append(&search);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(true);
        list.add_css_class("model-list");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&list));
        scroll.set_min_content_height(200);
        scroll.set_max_content_height(200);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.add_css_class("model-scroll");
        root.append(&scroll);

        let ml = ModelList {
            root,
            list: list.clone(),
            search: search.clone(),
            banner,
            spinner,
            message,
            retry,
            count,
            pinned: Rc::new(RefCell::new(Vec::new())),
            order: Rc::new(RefCell::new(HashMap::new())),
            generation: Rc::new(Cell::new(0)),
            fill_gen: Rc::new(Cell::new(0)),
            current: Rc::new(RefCell::new(String::new())),
            on_pick: Rc::new(RefCell::new(None)),
        };

        // filter by showing and hiding rows: typing never rebuilds the list
        {
            let search2 = search.clone();
            list.set_filter_func(move |row| {
                let q = search2.text().to_lowercase();
                q.is_empty() || row.widget_name().to_lowercase().contains(q.as_str())
            });
            let list2 = list.clone();
            search.connect_search_changed(move |_| list2.invalidate_filter());
        }
        // pinned first (in the order they were pinned), then the provider's own order
        {
            let (pinned, order) = (ml.pinned.clone(), ml.order.clone());
            list.set_sort_func(move |a, b| {
                let key = |r: &gtk::ListBoxRow| {
                    let name = r.widget_name();
                    let pin = pinned.borrow().iter().position(|m| m.as_str() == name.as_str());
                    match pin {
                        Some(i) => (0, i),
                        None => (1, order.borrow().get(name.as_str()).copied().unwrap_or(usize::MAX)),
                    }
                };
                key(a).cmp(&key(b)).into()
            });
        }
        {
            let pinned = ml.pinned.clone();
            list.set_header_func(move |row, before| {
                let is_pin = |r: &gtk::ListBoxRow| pinned.borrow().iter().any(|m| m.as_str() == r.widget_name().as_str());
                let any = !pinned.borrow().is_empty();
                let mine = is_pin(row);
                let theirs = before.map(is_pin);
                let header = match (before.is_none(), theirs) {
                    (true, _) if mine => Some("PINNED"),
                    (true, _) if any => Some("ALL MODELS"),
                    (false, Some(true)) if !mine => Some("ALL MODELS"),
                    _ => None,
                };
                match header {
                    Some(t) => row.set_header(Some(&lbl(t, "pop-head"))),
                    None => row.set_header(None::<&gtk::Widget>),
                }
            });
        }
        {
            let ml2 = ml.clone();
            list.connect_row_activated(move |_, row| {
                let name = row.widget_name().to_string();
                let f = ml2.on_pick.borrow().clone();
                if let Some(f) = f {
                    f(&name);
                }
            });
        }
        let _ = reload;
        ml
    }

    fn set_loading(&self, text: &str) {
        self.banner.set_visible(true);
        self.spinner.set_visible(true);
        self.spinner.start();
        self.message.set_text(text);
        self.retry.set_visible(false);
    }

    fn set_note(&self, text: &str, retry: bool) {
        self.spinner.stop();
        self.spinner.set_visible(false);
        self.banner.set_visible(!text.is_empty());
        self.message.set_text(text);
        self.retry.set_visible(retry);
    }

    fn mark_current(&self, model: &str) {
        *self.current.borrow_mut() = model.to_string();
        let mut c = self.list.first_child();
        while let Some(w) = c {
            if let Some(row) = w.downcast_ref::<gtk::ListBoxRow>() {
                if let Some(tick) = row.child().and_then(|b| b.last_child()) {
                    tick.set_visible(row.widget_name().as_str() == model);
                }
            }
            c = w.next_sibling();
        }
    }

    fn star_icon(on: bool) -> &'static str {
        if on { "starred-symbolic" } else { "non-starred-symbolic" }
    }

    /// Fills the list: the pinned models (even when the provider could not be asked) and the provider's list. Rows are added a
    /// batch at a time so a list of hundreds does not freeze the window.
    fn fill(&self, fetched: &[String]) {
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let mut names: Vec<String> = self.pinned.borrow().clone();
        for m in fetched {
            if !names.contains(m) {
                names.push(m.clone());
            }
        }
        *self.order.borrow_mut() = names.iter().enumerate().map(|(i, m)| (m.clone(), i)).collect();
        self.count.set_text(&if fetched.is_empty() { String::new() } else { format!("{} models", fetched.len()) });
        let ml = self.clone();
        let gen = self.fill_gen.get().wrapping_add(1);
        self.fill_gen.set(gen);
        let todo = Rc::new(RefCell::new(names.into_iter()));
        glib::idle_add_local(move || {
            if ml.fill_gen.get() != gen {
                return glib::ControlFlow::Break;
            }
            for _ in 0..60 {
                let Some(m) = todo.borrow_mut().next() else {
                    ml.list.invalidate_sort();
                    ml.list.invalidate_headers();
                    let cur = ml.current.borrow().clone();
                    ml.mark_current(&cur);
                    return glib::ControlFlow::Break;
                };
                ml.list.append(&ml.row(&m));
            }
            ml.list.invalidate_sort();
            ml.list.invalidate_headers();
            glib::ControlFlow::Continue
        });
    }

    fn row(&self, model: &str) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(model);
        row.add_css_class("model-row");
        let line = hbox(8);
        let star = crate::icon_button(Self::star_icon(self.pinned.borrow().iter().any(|m| m == model)), "Pin to the model menu", "icon-btn star-btn");
        {
            let (ml, model2, star2) = (self.clone(), model.to_string(), star.clone());
            star.connect_clicked(move |_| {
                let model = model2.clone();
                let now_pinned = {
                    let mut p = ml.pinned.borrow_mut();
                    if let Some(i) = p.iter().position(|m| *m == model) {
                        p.remove(i);
                        false
                    } else {
                        p.push(model.clone());
                        true
                    }
                };
                star2.set_icon_name(Self::star_icon(now_pinned));
                star2.set_tooltip_text(Some(if now_pinned { "Unpin from the model menu" } else { "Pin to the model menu" }));
                ml.list.invalidate_sort();
                ml.list.invalidate_headers();
            });
            star.set_tooltip_text(Some(if self.pinned.borrow().iter().any(|m| *m == model) { "Unpin from the model menu" } else { "Pin to the model menu" }));
        }
        line.append(&star);
        let name = lbl(&chat::nice_name(model), "pop-title");
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        line.append(&name);
        let tick = crate::icon("object-select-symbolic", 16);
        tick.add_css_class("pop-check");
        tick.set_visible(false);
        line.append(&tick);
        row.set_child(Some(&line));
        row
    }
}

// ---------------------------------------------------------------- the chooser

pub fn models_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = sheet(ui, "Model and API key", 620);
    let sel = Selection::load();
    let shown = Rc::new(RefCell::new(visible_providers(&sel.provider)));

    let names: Vec<&str> = shown.borrow().iter().map(|p| p.name).collect();
    let provider_dd = gtk::DropDown::from_strings(&names);
    provider_dd.set_selected(shown.borrow().iter().position(|p| p.id == sel.provider).unwrap_or(0) as u32);
    body.append(&crate::field("Provider", &provider_dd));
    let hidden_n = providers::all().len() - shown.borrow().len();
    if hidden_n > 0 {
        body.append(&lbl(&format!("{hidden_n} providers are hidden. Settings > Model and profiles has a switch for each."), "field-hint"));
    }

    let key = gtk::PasswordEntry::new();
    key.set_show_peek_icon(true);
    key.add_css_class("field-input");
    let key_field = crate::field("API key", &key);
    body.append(&key_field);

    // the address: always shown for custom and local servers, under "Advanced" for the others
    let url = gtk::Entry::new();
    url.add_css_class("field-input");
    url.set_hexpand(true);
    let test = secondary("Test connection");
    let url_row = hbox(8);
    url_row.append(&url);
    url_row.append(&test);
    let url_field = crate::field("Address", &url_row);
    let url_hint = lbl("", "field-hint");
    url_hint.set_wrap(true);
    url_field.append(&url_hint);
    // Can the model look at pictures? Normally decided from the model's name.
    let vision_dd = gtk::DropDown::from_strings(&["Decide from the model's name (recommended)", "This model can see pictures", "This model cannot see pictures"]);
    vision_dd.set_selected(match sel.vision {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    });
    url_field.append(&crate::field("Pictures", &vision_dd));

    let url_rev = gtk::Revealer::new();
    url_rev.set_child(Some(&url_field));
    let advanced = gtk::Button::with_label("Advanced: address and pictures");
    advanced.set_has_frame(false);
    advanced.set_halign(gtk::Align::Start);
    advanced.add_css_class("link-btn");
    {
        let r = url_rev.clone();
        advanced.connect_clicked(move |_| r.set_reveal_child(!r.reveals_child()));
    }
    body.append(&advanced);
    body.append(&url_rev);

    let model = gtk::Entry::new();
    model.add_css_class("field-input");
    model.set_placeholder_text(Some("Model name, or pick one below"));
    body.append(&crate::field("Model", &model));

    let ml = ModelList::new();
    body.append(&ml.root);

    let status = lbl("", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);

    let current = {
        let shown = shown.clone();
        move |dd: &gtk::DropDown| -> &'static Provider { shown.borrow()[(dd.selected() as usize).min(shown.borrow().len().saturating_sub(1))] }
    };

    // typing a model name moves the tick
    {
        let ml = ml.clone();
        model.connect_changed(move |e| ml.mark_current(e.text().trim()));
    }
    // a click on a row picks that model
    {
        let model = model.clone();
        *ml.on_pick.borrow_mut() = Some(Rc::new(move |m: &str| model.set_text(m)));
    }

    // (re)load the list for what is on screen now
    let load: Rc<dyn Fn(bool)> = {
        let (provider_dd, url, key, ml, current) = (provider_dd.clone(), url.clone(), key.clone(), ml.clone(), current.clone());
        Rc::new(move |force: bool| {
            let p = current(&provider_dd);
            let gen = ml.generation.get().wrapping_add(1);
            ml.generation.set(gen);
            let base = url.text().trim().to_string();
            let typed = Some(key.text().to_string()).filter(|k| !k.trim().is_empty());
            ml.fill(&[]);
            ml.set_loading("Loading the model list…");
            let ml2 = ml.clone();
            fetch_models(p, base, typed, force, move |r| {
                if ml2.generation.get() != gen {
                    return;
                }
                match r {
                    Ok(list) if list.is_empty() => {
                        ml2.fill(&[]);
                        ml2.set_note("The provider returned no models. You can still type a model name above.", true);
                    }
                    Ok(list) => {
                        ml2.fill(&list);
                        ml2.set_note("", false);
                    }
                    Err(e) => {
                        ml2.fill(&[]);
                        ml2.set_note(&e, true);
                    }
                }
            });
        })
    };
    {
        let load = load.clone();
        ml.retry.connect_clicked(move |_| load(true));
    }
    {
        // the refresh icon in the list's heading
        let load = load.clone();
        if let Some(b) = ml.root.first_child().and_then(|h| h.last_child()).and_then(|b| b.downcast::<gtk::Button>().ok()) {
            b.connect_clicked(move |_| load(true));
        }
    }
    {
        let (load, key) = (load.clone(), key.clone());
        key.connect_activate(move |_| load(true));
    }
    {
        let (load, url) = (load.clone(), url.clone());
        url.connect_activate(move |_| load(true));
    }

    // test the connection
    {
        let (provider_dd, url, key, status, current) = (provider_dd.clone(), url.clone(), key.clone(), status.clone(), current.clone());
        let load = load.clone();
        test.connect_clicked(move |b| {
            let p = current(&provider_dd);
            let typed = Some(key.text().to_string()).filter(|k| !k.trim().is_empty());
            status.set_text("Testing the connection…");
            b.set_sensitive(false);
            let (status, b, load) = (status.clone(), b.clone(), load.clone());
            test_connection(p, url.text().trim().to_string(), typed, move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(m) => {
                        status.set_text(&m);
                        load(false);
                    }
                    Err(e) => status.set_text(&e),
                }
            });
        });
    }

    // fill the fields for the provider that is selected
    let refresh = {
        let (key, key_field, url, url_hint, advanced, url_rev, model, status, ml, load) = (key.clone(), key_field.clone(), url.clone(), url_hint.clone(), advanced.clone(), url_rev.clone(), model.clone(), status.clone(), ml.clone(), load.clone());
        let sel = Rc::new(RefCell::new(sel.clone()));
        let current = current.clone();
        move |dd: &gtk::DropDown| {
            let p = current(dd);
            let s = sel.borrow();
            url.set_text(&s.base_url_for(p));
            key_field.set_visible(!p.local);
            url_rev.set_visible(p.protocol != Protocol::ClaudeCode);
            url_rev.set_reveal_child(needs_url(p));
            advanced.set_visible(p.protocol != Protocol::ClaudeCode && !needs_url(p));
            url_hint.set_text(if p.id == "custom" { "The address of any OpenAI-compatible server, for example http://192.168.1.20:8000/v1 or https://my-host.example/v1." } else if needs_url(p) { "Where the server runs. The default is right unless you changed its port." } else { "Only change this for a proxy or a regional endpoint." });
            key.set_text("");
            key.set_placeholder_text(Some("Paste the key (a saved one is used when this stays empty)"));
            model.set_text(if s.provider == p.id { &s.model } else { p.models.first().copied().unwrap_or("") });
            *ml.pinned.borrow_mut() = pins_of(&s, p);
            ml.search.set_text("");
            status.set_text(if p.protocol == Protocol::ClaudeCode {
                if zo_core::ai::claude_code::binary().is_some() { "Uses the Claude Code program you installed and signed in to yourself (claude auth login). ZO-AI never sees your login. Experimental. The full model list is read with an Anthropic API key (add one under the Anthropic provider or in a profile); without one you get Claude Code's own names for the newest models." } else { "Claude Code is not installed. Install it with: npm install -g @anthropic-ai/claude-code, then sign in with: claude auth login" }
            } else if p.local { "Runs on this computer. No key, and nothing leaves it." } else if keys::available() { "The key is stored in the system keyring." } else { "The keyring tool (secret-tool) is missing, so keys cannot be saved." });
            ml.mark_current(&model.text());
            load(false);
        }
    };
    refresh(&provider_dd);
    provider_dd.connect_selected_notify(move |dd| refresh(dd));

    // show which local servers are already running
    {
        let status = status.clone();
        glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(|| agent::detect_local().iter().map(|p| p.name.to_string()).collect::<Vec<_>>()).await.unwrap_or_default();
            if !found.is_empty() {
                let t = status.text();
                status.set_text(&format!("{t}\nFound running here: {}", found.join(", ")));
            }
        });
    }

    let hint = lbl("Stars are saved with Save.", "field-hint");
    hint.set_hexpand(true);
    let cancel = secondary("Cancel");
    let save = primary("Save");
    foot.append(&hint);
    foot.append(&cancel);
    foot.append(&save);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    {
        let (w, ui2, provider_dd, key, url, model, status, ml, vision_dd, current) = (win.clone(), ui.clone(), provider_dd.clone(), key.clone(), url.clone(), model.clone(), status.clone(), ml.clone(), vision_dd.clone(), current.clone());
        save.connect_clicked(move |_| {
            let mut sel = Selection::load();
            sel.vision = match vision_dd.selected() {
                1 => Some(true),
                2 => Some(false),
                _ => None,
            };
            let p = current(&provider_dd);
            // a profile belongs to one provider: choosing another one leaves the profile as it was
            if let Some(id) = sel.profile.clone() {
                if profiles::load().iter().any(|x| x.id == id && x.provider != p.id) {
                    sel.profile = None;
                }
            }
            let typed = key.text().to_string();
            if !typed.trim().is_empty() {
                // with a profile chosen the key belongs to that profile
                if let Err(e) = keys::set(&profiles::key_id(&sel, p.id), &typed) {
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
            profiles::update_active(&sel);
            // the stars: the active profile's pins become exactly what is starred (a profile is made when the first star is saved)
            let want = ml.pinned.borrow().clone();
            let have = pins_of(&sel, p);
            if !want.is_empty() || !have.is_empty() {
                let id = if want.is_empty() { sel.profile.clone() } else { profiles::ensure_active(&mut sel, p.name).ok() };
                if let Some(id) = id {
                    sel.save();
                    for m in have.iter().filter(|m| !want.contains(m)) {
                        profiles::unpin(&id, m);
                    }
                    for m in want.iter().filter(|m| !have.contains(m)) {
                        profiles::pin(&id, m);
                    }
                }
            }
            ui2.model_label.set_text(&chat::pill_text(&sel));
            ui2.pv_model_label.set_text(&chat::pill_text(&sel));
            w.close();
        });
    }
    // this chooser is tall: let it use most of the window (the list inside scrolls on its own)
    if let Some(sw) = body.ancestor(gtk::ScrolledWindow::static_type()).and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok()) {
        let h = ui.window.height().max(ui.window.default_height()).max(600);
        sw.set_max_content_height((h as f64 * 0.8) as i32);
    }
    win.present();
}

// ---------------------------------------------------------------- a custom endpoint, as a profile

/// Name, address, key and model: makes a profile for any OpenAI-compatible server and switches to it. `done` runs afterwards.
pub fn add_endpoint_sheet(ui: &Rc<Ui>, done: impl Fn() + 'static) {
    let (win, body, foot) = sheet(ui, "Add a custom endpoint", 520);
    let name = gtk::Entry::new();
    let name_focus = name.clone();
    name.add_css_class("field-input");
    name.set_placeholder_text(Some("Work server, Home GPU…"));
    body.append(&crate::field("Name", &name));
    let url = gtk::Entry::new();
    url.add_css_class("field-input");
    url.set_placeholder_text(Some("https://my-host.example/v1"));
    body.append(&crate::field("Address", &url));
    body.append(&lbl("Any server that speaks the OpenAI chat API: vLLM, LM Studio, a company gateway, a proxy.", "field-hint"));
    let key = gtk::PasswordEntry::new();
    key.set_show_peek_icon(true);
    key.add_css_class("field-input");
    body.append(&crate::field("API key (leave empty if it needs none)", &key));
    let model = gtk::Entry::new();
    model.add_css_class("field-input");
    model.set_placeholder_text(Some("Model name (Test connection can list them)"));
    body.append(&crate::field("Model", &model));
    let status = lbl("", "field-hint");
    status.set_wrap(true);
    body.append(&status);

    let test = secondary("Test connection");
    let cancel = secondary("Cancel");
    let save = primary("Add and use");
    foot.append(&test);
    foot.append(&cancel);
    foot.append(&save);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    {
        let (url, key, model, status) = (url.clone(), key.clone(), model.clone(), status.clone());
        test.connect_clicked(move |b| {
            let Some(p) = providers::find("custom") else { return };
            if url.text().trim().is_empty() {
                status.set_text("Type the address first.");
                return;
            }
            status.set_text("Testing the connection…");
            b.set_sensitive(false);
            let typed = Some(key.text().to_string()).filter(|k| !k.trim().is_empty());
            let (status, b, model) = (status.clone(), b.clone(), model.clone());
            // a custom endpoint is asked without a key when none is typed: some servers need none
            fetch_models(p, url.text().trim().to_string(), typed, true, move |r| {
                b.set_sensitive(true);
                match r {
                    Ok(list) => {
                        if model.text().trim().is_empty() {
                            if let Some(m) = list.first() {
                                model.set_text(m);
                            }
                        }
                        status.set_text(&format!("Connected. {} models available.", list.len()));
                    }
                    Err(e) => status.set_text(&e),
                }
            });
        });
    }
    {
        let (w, ui2) = (win.clone(), ui.clone());
        save.connect_clicked(move |_| {
            let (n, address, m) = (name.text().trim().to_string(), url.text().trim().to_string(), model.text().trim().to_string());
            if n.is_empty() || address.is_empty() {
                status.set_text("A name and an address are needed.");
                return;
            }
            if !(address.starts_with("http://") || address.starts_with("https://")) {
                status.set_text("The address should start with http:// or https://");
                return;
            }
            let mut sel = Selection::load();
            sel.profile = None; // do not borrow another profile's key
            sel.provider = "custom".to_string();
            sel.model = m.clone();
            sel.base_urls.insert("custom".to_string(), address);
            let p = match profiles::create(&n, &sel) {
                Ok(p) => p,
                Err(e) => {
                    status.set_text(&e);
                    return;
                }
            };
            let kid = format!("custom@{}", p.id);
            let typed = key.text().to_string();
            if typed.trim().is_empty() {
                keys::clear(&kid); // the new profile must not carry the shared custom key to another address
            } else if let Err(e) = keys::set(&kid, &typed) {
                status.set_text(&e);
                return;
            }
            if !m.is_empty() {
                profiles::pin(&p.id, &m);
            }
            let p = profiles::load().into_iter().find(|x| x.id == p.id).unwrap_or(p);
            let mut s = Selection::load();
            s.apply_profile(&p);
            s.save();
            ui2.model_label.set_text(&chat::pill_text(&s));
            ui2.pv_model_label.set_text(&chat::pill_text(&s));
            w.close();
            done();
        });
    }
    win.present();
    name_focus.grab_focus();
}
