//! The chat: sending a message to the chosen model, showing what it says and the tools it runs,
//! and the sheet where the person picks a provider, a model and an API key.

use crate::{hbox, lbl, primary, secondary, sheet, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
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
    busy: Cell<bool>,
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
        Rc::new(Ctl { chat: chat.clone(), hero: hero.clone(), thread, scroll, history: RefCell::new(Vec::new()), session: RefCell::new(None), busy: Cell::new(false) })
    }

    pub fn new_conversation(&self) {
        if self.busy.get() {
            return;
        }
        self.history.borrow_mut().clear();
        *self.session.borrow_mut() = None;
        while let Some(c) = self.thread.first_child() {
            self.thread.remove(&c);
        }
        self.scroll.set_visible(false);
        self.hero.set_visible(true);
        self.chat.set_valign(gtk::Align::Center);
    }

    fn add(&self, text: &str, class: &str) -> gtk::Label {
        let l = lbl(text, class);
        l.set_wrap(true);
        l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        l.set_xalign(0.0);
        l.set_selectable(true);
        l.set_halign(if class == "msg-user" { gtk::Align::End } else { gtk::Align::Fill });
        self.thread.append(&l);
        let scroll = self.scroll.clone();
        glib::timeout_add_local_once(Duration::from_millis(40), move || {
            let a = scroll.vadjustment();
            a.set_value(a.upper() - a.page_size());
        });
        l
    }

    pub fn submit(self: &Rc<Self>, prompt: &gtk::Entry) {
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

        let claude = sel.provider == "claude-code";
        let cfg = if claude {
            None
        } else {
            match Config::from_selection(&sel, agent::SYSTEM_PROMPT) {
                Ok(c) => Some(c),
                Err(e) => {
                    self.add(&e, "msg-error");
                    return;
                }
            }
        };
        let model = if sel.model.is_empty() { "sonnet".to_string() } else { sel.model.clone() };
        self.busy.set(true);
        prompt.set_sensitive(false);
        let thinking = self.add("Thinking…", "msg-tool");
        let (tx, rx) = mpsc::channel::<Msg>();
        let mut history = std::mem::take(&mut *self.history.borrow_mut());
        let mut session = self.session.borrow().clone();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let mut emit = |e: Event| {
                let _ = tx2.send(Msg::Ev(e));
            };
            let r = match &cfg {
                None => claude_code::run(&model, &mut session, &text, &mut emit),
                Some(cfg) => agent::run(cfg, &mut history, &text, zo_core::Mode::load(), &zo_core::approval::AppApprover, &mut emit),
            };
            let _ = tx.send(Msg::Done(r, history, session));
        });
        let me = self.clone();
        let prompt = prompt.clone();
        glib::timeout_add_local(Duration::from_millis(40), move || {
            loop {
                match rx.try_recv() {
                    Ok(Msg::Ev(Event::Text(t))) => {
                        thinking.set_visible(false);
                        me.add(&t, "msg-ai");
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
                        *me.history.borrow_mut() = history;
                        *me.session.borrow_mut() = session;
                        if let Err(e) = result {
                            me.add(&e, "msg-error");
                        }
                        me.busy.set(false);
                        prompt.set_sensitive(true);
                        prompt.grab_focus();
                        return glib::ControlFlow::Break;
                    }
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        me.busy.set(false);
                        prompt.set_sensitive(true);
                        return glib::ControlFlow::Break;
                    }
                }
            }
        });
    }
}

/// What the model pill says.
pub fn pill_text(sel: &Selection) -> String {
    match providers::find(&sel.provider) {
        Some(_) if !sel.model.is_empty() => sel.model.clone(),
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
            w.close();
        });
    }
    win.present();
}

