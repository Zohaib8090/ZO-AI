//! The chat: sending a message to the chosen model, showing what it says and the tools it runs,
//! and the sheet where the person picks a provider, a model and an API key.

use crate::chats::{self, Branch, Chat, Entry, Version};
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
use zo_core::ai::{keys, profiles, providers, Selection};

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
    /// Called with (which of your messages, its new text) when a message is edited and sent again.
    on_edit: Rc<RefCell<Option<Rc<dyn Fn(usize, String)>>>>,
    last_retry: RefCell<Option<gtk::Button>>,
    /// The takes of your messages (see `chats::Branch`); the one in use is the conversation that is on screen.
    branches: RefCell<std::collections::BTreeMap<usize, Branch>>,
    on_switch: Rc<RefCell<Option<Rc<dyn Fn(usize, i32)>>>>,
    last_nav: RefCell<Option<gtk::Widget>>,
    /// What the provider said the last request used.
    last_usage: Cell<Option<zo_core::ai::wire::Usage>>,
    /// The box that gathers the tool runs of the current turn ("Running a task in the terminal…", then "Ran 3 commands").
    group: RefCell<Option<Group>>,
}

/// One collapsed box of tool runs in the conversation.
struct Group {
    frame: gtk::Box,
    head: gtk::Label,
    spinner: gtk::Spinner,
    body: gtk::Box,
    count: Cell<usize>,
}

enum Msg {
    Ev(Event),
    Done(Result<(), String>, Vec<Turn>, Option<String>),
}

impl Ctl {
    /// Adds the message list between the greeting and the composer.
    pub fn new(chat: &gtk::Box, hero: &gtk::Box) -> Rc<Ctl> {
        let thread = vbox(10);
        thread.add_css_class("thread");
        // The conversation is a centred column: it never grows wider than 760 px, and keeps clear of the window's edges.
        let column = adw::Clamp::new();
        {
            let col = column.clone();
            crate::theme::on_zoom(move |_| {
                col.set_maximum_size(crate::theme::zs(760));
                col.set_tightening_threshold(crate::theme::zs(700));
            });
        }
        column.set_margin_start(24);
        column.set_margin_end(24);
        column.set_child(Some(&thread));
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&column));
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_visible(false);
        chat.insert_child_after(&scroll, Some(hero));
        Rc::new(Ctl { chat: chat.clone(), hero: hero.clone(), thread, scroll, history: RefCell::new(Vec::new()), session: RefCell::new(None), extra: RefCell::new(String::new()), title: RefCell::new(String::new()), id: RefCell::new(String::new()), entries: RefCell::new(Vec::new()), on_saved: RefCell::new(None), busy: Cell::new(false), cancel: Arc::new(AtomicBool::new(false)), on_busy: RefCell::new(None), on_retry: Rc::new(RefCell::new(None)), on_edit: Rc::new(RefCell::new(None)), last_retry: RefCell::new(None), branches: RefCell::new(Default::default()), on_switch: Rc::new(RefCell::new(None)), last_nav: RefCell::new(None), last_usage: Cell::new(None), group: RefCell::new(None) })
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
            branches: self.snapshot_branches(),
        });
        zo_core::ai::memory_agent::after_turn(&self.id.borrow(), &self.title.borrow(), &self.entries.borrow());
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
        *self.branches.borrow_mut() = chat.branches;
        self.hero.set_visible(false);
        self.scroll.set_visible(true);
        self.chat.set_valign(gtk::Align::Fill);
        for e in chat.entries {
            self.add(&e.text, &e.kind);
        }
        self.end_group();
    }

    /// The conversation as the model gets it (for the Context panel and for compacting).
    pub fn history_snapshot(&self) -> Vec<Turn> {
        self.history.borrow().clone()
    }

    /// The project instructions, memory and attached context that go with every message.
    pub fn extra_text(&self) -> String {
        self.extra.borrow().clone()
    }

    pub fn usage(&self) -> Option<zo_core::ai::wire::Usage> {
        self.last_usage.get()
    }

    /// Summarizes the older part of this conversation now (the Compact button). `done` gets the outcome on the main thread.
    pub fn compact_now(self: &Rc<Self>, done: Rc<dyn Fn(Result<(), String>)>) {
        if self.busy.get() {
            done(Err("Wait until the answer is finished.".into()));
            return;
        }
        let sel = Selection::load();
        if sel.provider == "claude-code" {
            done(Err("Claude Code keeps and compacts this conversation itself.".into()));
            return;
        }
        let system = format!("{}\n\n{}", agent::SYSTEM_PROMPT, self.extra.borrow());
        let cfg = match Config::from_selection(&sel, &system) {
            Ok(c) => c,
            Err(e) => {
                done(Err(e));
                return;
            }
        };
        self.set_busy(true);
        let mut history = self.history.borrow().clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = agent::compact(&cfg, &mut history, &AtomicBool::new(false), &mut |_| {}).map(|_| history);
            let _ = tx.send(r);
        });
        let me = self.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || match rx.try_recv() {
            Ok(Ok(h)) => {
                *me.history.borrow_mut() = h;
                me.add("The earlier part of this conversation was summarized, so there is more room to go on.", "msg-tool");
                me.persist();
                me.set_busy(false);
                done(Ok(()));
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                me.set_busy(false);
                done(Err(e));
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                me.set_busy(false);
                glib::ControlFlow::Break
            }
        });
    }

    /// The whole conversation as plain text (your messages and the replies), for copying.
    pub fn transcript(&self) -> String {
        self.entries
            .borrow()
            .iter()
            .filter_map(|e| match e.kind.as_str() {
                "msg-user" => Some(format!("You: {}", e.text)),
                "msg-ai" => Some(format!("ZO-AI: {}", e.text)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Called when "try again" is pressed.
    pub fn on_retry(&self, f: Rc<dyn Fn()>) {
        *self.on_retry.borrow_mut() = Some(f);
    }

    /// Called with (which of your messages, -1 or 1) when an arrow under it is pressed.
    pub fn on_switch(&self, f: Rc<dyn Fn(usize, i32)>) {
        *self.on_switch.borrow_mut() = Some(f);
    }

    pub fn on_edit(&self, f: Rc<dyn Fn(usize, String)>) {
        *self.on_edit.borrow_mut() = Some(f);
    }

    /// Throws away the last reply and asks the same thing again.
    pub fn retry(self: &Rc<Self>, prompt: &Prompt) {
        let last = self.entries.borrow().iter().filter(|e| e.kind == "msg-user").count();
        if last > 0 {
            self.resend_from(last - 1, None, prompt);
        }
    }

    /// Goes back to your `nth` message (counting from 0): everything from it on is removed, and it is asked again, with
    /// `new_text` instead of its own words when it was edited.
    pub fn resend_from(self: &Rc<Self>, nth: usize, new_text: Option<String>, prompt: &Prompt) {
        if self.busy.get() {
            return;
        }
        let entries = self.entries.borrow().clone();
        let Some(at) = entries.iter().enumerate().filter(|(_, e)| e.kind == "msg-user").map(|(i, _)| i).nth(nth) else { return };
        let text = new_text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| entries[at].text.clone());
        let turn_at = self.history.borrow().iter().enumerate().filter(|(_, t)| matches!(t, Turn::User(_) | Turn::UserImages(..))).map(|(i, _)| i).nth(nth);
        // what is there now is kept as one take of this message, and a new take starts
        self.fork(nth, at, turn_at.unwrap_or(self.history.borrow().len()));
        if let Some(p) = turn_at {
            self.history.borrow_mut().truncate(p);
        }
        // an earlier message was changed: Claude Code's own memory of the conversation cannot go back, so it starts over
        let is_last = entries.iter().filter(|e| e.kind == "msg-user").count() == nth + 1;
        if !is_last {
            *self.session.borrow_mut() = None;
        }
        // show the chat again without that message and what followed it
        self.group.borrow_mut().take();
        while let Some(c) = self.thread.first_child() {
            self.thread.remove(&c);
        }
        *self.last_retry.borrow_mut() = None;
        self.entries.borrow_mut().clear();
        for e in &entries[..at] {
            self.add(&e.text, &e.kind);
        }
        self.end_group();
        // whatever was being typed in the box stays
        let draft = prompt.text();
        prompt.set_text(&text);
        self.submit(prompt);
        prompt.set_text(&draft);
    }

    /// Keeps the conversation from message `nth` on as a take of its own and opens a new, empty one in its place.
    fn fork(&self, nth: usize, entry_at: usize, turn_at: usize) {
        let tail = Version {
            entries: self.entries.borrow().get(entry_at..).map(<[_]>::to_vec).unwrap_or_default(),
            history: self.history.borrow().get(turn_at..).map(<[_]>::to_vec).unwrap_or_default(),
            // forks made later inside this take go with it
            sub: {
                let mut live = self.branches.borrow_mut();
                let later: Vec<usize> = live.range(nth + 1..).map(|(k, _)| *k).collect();
                later.into_iter().filter_map(|k| live.remove(&k).map(|b| (k, b))).collect()
            },
        };
        let mut live = self.branches.borrow_mut();
        let b = live.entry(nth).or_insert_with(|| Branch { versions: vec![Version::default()], current: 0 });
        let cur = b.current;
        b.versions[cur] = tail;
        b.versions.push(Version::default());
        b.current = b.versions.len() - 1;
    }

    /// The entry and turn at which your message number `nth` starts.
    fn tail_start(&self, nth: usize) -> Option<(usize, usize)> {
        let at = self.entries.borrow().iter().enumerate().filter(|(_, e)| e.kind == "msg-user").map(|(i, _)| i).nth(nth)?;
        let turn_at = self.history.borrow().iter().enumerate().filter(|(_, t)| matches!(t, Turn::User(_) | Turn::UserImages(..))).map(|(i, _)| i).nth(nth).unwrap_or(self.history.borrow().len());
        Some((at, turn_at))
    }

    /// Shows an other take of your message number `nth`: `delta` is -1 for the one before and 1 for the one after.
    pub fn switch_version(self: &Rc<Self>, nth: usize, delta: i32) {
        if self.busy.get() {
            return;
        }
        let Some((at, turn_at)) = self.tail_start(nth) else { return };
        let (cur, count) = match self.branches.borrow().get(&nth) {
            Some(b) => (b.current, b.versions.len()),
            None => return,
        };
        let next = cur as i32 + delta;
        if next < 0 || next as usize >= count {
            return;
        }
        let next = next as usize;
        // put the take on screen away, and bring the other one in
        let mut live = self.branches.borrow_mut();
        let later: Vec<usize> = live.range(nth + 1..).map(|(k, _)| *k).collect();
        let sub = later.into_iter().filter_map(|k| live.remove(&k).map(|b| (k, b))).collect();
        let outgoing = Version { entries: self.entries.borrow()[at..].to_vec(), history: self.history.borrow().get(turn_at..).map(<[_]>::to_vec).unwrap_or_default(), sub };
        let b = live.get_mut(&nth).expect("checked above");
        b.versions[cur] = outgoing;
        let incoming = b.versions[next].clone();
        b.current = next;
        live.extend(incoming.sub);
        drop(live);
        self.entries.borrow_mut().truncate(at);
        self.entries.borrow_mut().extend(incoming.entries);
        let mut h = self.history.borrow_mut();
        h.truncate(turn_at);
        h.extend(incoming.history);
        drop(h);
        // Claude Code's own memory of the conversation cannot follow a switch
        *self.session.borrow_mut() = None;
        self.redraw();
        self.persist();
    }

    /// Draws the conversation again from what is stored.
    fn redraw(&self) {
        let entries = self.entries.borrow().clone();
        self.group.borrow_mut().take();
        while let Some(c) = self.thread.first_child() {
            self.thread.remove(&c);
        }
        *self.last_retry.borrow_mut() = None;
        *self.last_nav.borrow_mut() = None;
        self.entries.borrow_mut().clear();
        for e in &entries {
            self.add(&e.text, &e.kind);
        }
        self.end_group();
    }

    /// The takes as they are saved: the one on screen is written into its place.
    fn snapshot_branches(&self) -> std::collections::BTreeMap<usize, Branch> {
        let mut snap = self.branches.borrow().clone();
        let keys: Vec<usize> = snap.keys().copied().collect();
        for k in keys.into_iter().rev() {
            let Some((at, turn_at)) = self.tail_start(k) else { continue };
            let sub: std::collections::BTreeMap<usize, Branch> = snap.range(k + 1..).map(|(i, b)| (*i, b.clone())).collect();
            let tail = Version { entries: self.entries.borrow()[at..].to_vec(), history: self.history.borrow().get(turn_at..).map(<[_]>::to_vec).unwrap_or_default(), sub };
            if let Some(b) = snap.get_mut(&k) {
                let cur = b.current.min(b.versions.len().saturating_sub(1));
                if cur < b.versions.len() {
                    b.versions[cur] = tail;
                }
            }
        }
        snap
    }

    /// "‹ 2 / 3 ›": shown under a message that has had more than one take; None when it has had one.
    fn version_nav(&self, nth: usize) -> Option<gtk::Box> {
        let (cur, count) = {
            let b = self.branches.borrow();
            let br = b.get(&nth)?;
            (br.current, br.versions.len())
        };
        if count < 2 {
            return None;
        }
        let nav = hbox(2);
        nav.add_css_class("version-nav");
        let prev = crate::icon_button("go-previous-symbolic", "The take before this one", "icon-btn");
        prev.set_sensitive(cur > 0);
        let next = crate::icon_button("go-next-symbolic", "The next take", "icon-btn");
        next.set_sensitive(cur + 1 < count);
        for (b, d) in [(&prev, -1), (&next, 1)] {
            let on_switch = self.on_switch.clone();
            b.connect_clicked(move |_| {
                let f = on_switch.borrow().clone();
                if let Some(f) = f {
                    f(nth, d);
                }
            });
        }
        nav.append(&prev);
        nav.append(&lbl(&format!("{} / {}", cur + 1, count), "version-count"));
        nav.append(&next);
        Some(nav)
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
        self.group.borrow_mut().take();
        self.branches.borrow_mut().clear();
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

    /// Is this line one of the tool runs (the command that started, what came back, or its failure)?
    fn is_tool_line(text: &str, class: &str) -> bool {
        (class == "msg-tool" && text.starts_with("▸ ")) || class == "msg-tool-out" || (class == "msg-error" && text.starts_with("✗ "))
    }

    /// Puts a tool line into the current box of tool runs (making the box first if this is the first one of the turn).
    fn tool_line(&self, text: &str, class: &str) -> gtk::Widget {
        if self.group.borrow().is_none() {
            let frame = vbox(0);
            frame.add_css_class("tool-group");
            frame.add_css_class("active");
            let head_btn = gtk::Button::new();
            head_btn.add_css_class("flat");
            head_btn.add_css_class("tool-head");
            head_btn.set_halign(gtk::Align::Start);
            let row = hbox(8);
            let arrow = gtk::Image::from_icon_name("pan-end-symbolic");
            let head = lbl("Working on your computer…", "tool-head-text");
            let spinner = gtk::Spinner::new();
            spinner.start();
            row.append(&arrow);
            row.append(&head);
            row.append(&spinner);
            head_btn.set_child(Some(&row));
            let reveal = gtk::Revealer::new();
            reveal.set_transition_type(gtk::RevealerTransitionType::SlideDown);
            reveal.set_reveal_child(false);
            let body = vbox(2);
            body.add_css_class("tool-body");
            reveal.set_child(Some(&body));
            {
                let reveal = reveal.clone();
                head_btn.connect_clicked(move |_| {
                    let open = !reveal.reveals_child();
                    reveal.set_reveal_child(open);
                    arrow.set_icon_name(Some(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }));
                });
            }
            frame.append(&head_btn);
            frame.append(&reveal);
            self.thread.append(&frame);
            *self.group.borrow_mut() = Some(Group { frame, head, spinner, body, count: Cell::new(0) });
        }
        let g = self.group.borrow();
        let g = g.as_ref().expect("group was just made");
        if class == "msg-tool" {
            g.count.set(g.count.get() + 1);
            let shell = text.contains("zohara_shell");
            g.head.set_text(if shell { "Running a task in the terminal…" } else { "Working on your computer…" });
        }
        let l = lbl(text, class);
        l.set_wrap(true);
        l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        l.set_xalign(0.0);
        l.set_selectable(true);
        g.body.append(&l);
        let scroll = self.scroll.clone();
        glib::timeout_add_local_once(Duration::from_millis(40), move || {
            let a = scroll.vadjustment();
            a.set_value(a.upper() - a.page_size());
        });
        g.frame.clone().upcast()
    }

    /// The turn moved on (an answer, a new question, or the end): the box stops showing "working" and says what was done.
    fn end_group(&self) {
        if let Some(g) = self.group.borrow_mut().take() {
            g.frame.remove_css_class("active");
            g.spinner.stop();
            g.spinner.set_visible(false);
            let n = g.count.get();
            g.head.set_text(&if n == 1 { "Ran 1 command".to_string() } else { format!("Ran {n} commands") });
        }
    }

    fn add_plain(&self, text: &str, class: &str) -> gtk::Widget {
        if Self::is_tool_line(text, class) {
            return self.tool_line(text, class);
        }
        self.end_group();
        let w: gtk::Widget = if class == "msg-ai" {
            // a reply: formatted, with a copy button under it
            let col = vbox(2);
            col.add_css_class("msg-ai-wrap");
            col.append(&crate::markdown::render(text));
            let actions = hbox(2);
            actions.add_css_class("msg-actions");
            // the takes of the message this reply answers
            let nth = self.entries.borrow().iter().filter(|e| e.kind == "msg-user").count().saturating_sub(1);
            if let Some(nav) = self.version_nav(nth) {
                actions.append(&nav);
                if let Some(old) = self.last_nav.borrow_mut().replace(nav.clone().upcast()) {
                    old.set_visible(false);
                }
            }
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
        } else if class == "msg-user" {
            // your message: copy and edit sit under it (they show when the pointer is over it)
            let nth = self.entries.borrow().iter().filter(|e| e.kind == "msg-user").count().saturating_sub(1);
            let wrap = vbox(2);
            wrap.add_css_class("msg-user-wrap");
            wrap.set_halign(gtk::Align::End);
            let bubble = lbl(text, class);
            bubble.set_wrap(true);
            bubble.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            bubble.set_xalign(0.0);
            bubble.set_selectable(true);
            bubble.set_halign(gtk::Align::End);
            wrap.append(&bubble);
            let actions = hbox(2);
            actions.add_css_class("msg-actions");
            actions.set_halign(gtk::Align::End);
            // ‹ 2 / 3 › when this message has had more than one take (asked again, or edited)
            if let Some(nav) = self.version_nav(nth) {
                actions.append(&nav);
            }
            let copy = crate::icon_button("edit-copy-symbolic", "Copy your message", "icon-btn");
            let raw = text.to_string();
            copy.connect_clicked(move |b| b.clipboard().set_text(&raw));
            actions.append(&copy);
            let edit = crate::icon_button("document-edit-symbolic", "Edit this message and send it again", "icon-btn");
            actions.append(&edit);
            wrap.append(&actions);
            // the editor replaces the bubble while you change the words
            let editor = vbox(6);
            editor.add_css_class("msg-edit");
            editor.set_visible(false);
            let view = gtk::TextView::new();
            view.set_wrap_mode(gtk::WrapMode::WordChar);
            view.set_accepts_tab(false);
            view.set_size_request(420, -1);
            view.add_css_class("composer-entry");
            view.buffer().set_text(text);
            editor.append(&view);
            let buttons = hbox(8);
            buttons.set_halign(gtk::Align::End);
            let cancel = secondary("Cancel");
            let save = primary("Send");
            buttons.append(&cancel);
            buttons.append(&save);
            editor.append(&buttons);
            wrap.append(&editor);
            {
                let (bubble, actions, editor, view) = (bubble.clone(), actions.clone(), editor.clone(), view.clone());
                edit.connect_clicked(move |_| {
                    bubble.set_visible(false);
                    actions.set_visible(false);
                    editor.set_visible(true);
                    view.grab_focus();
                });
            }
            {
                let (bubble, actions, editor, view, raw) = (bubble.clone(), actions.clone(), editor.clone(), view.clone(), text.to_string());
                cancel.connect_clicked(move |_| {
                    view.buffer().set_text(&raw);
                    editor.set_visible(false);
                    bubble.set_visible(true);
                    actions.set_visible(true);
                });
            }
            {
                let (view, on_edit) = (view.clone(), self.on_edit.clone());
                save.connect_clicked(move |_| {
                    let b = view.buffer();
                    let new = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
                    let f = on_edit.borrow().clone();
                    if let Some(f) = f {
                        f(nth, new);
                    }
                });
            }
            wrap.upcast()
        } else if class == "msg-error" || (class == "msg-tool" && text == agent::STOPPED) {
            // something went wrong or it was stopped: the message, and a way to ask again
            let col = vbox(4);
            let l = lbl(text, class);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            l.set_xalign(0.0);
            l.set_selectable(true);
            col.append(&l);
            let retry = secondary("Try again");
            retry.set_halign(gtk::Align::Start);
            let on_retry = self.on_retry.clone();
            retry.connect_clicked(move |_| {
                let f = on_retry.borrow().clone();
                if let Some(f) = f {
                    f();
                }
            });
            col.append(&retry);
            if let Some(old) = self.last_retry.borrow_mut().replace(retry) {
                old.set_visible(false);
            }
            col.upcast()
        } else {
            let l = lbl(text, class);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            l.set_xalign(0.0);
            l.set_selectable(true);
            l.set_halign(gtk::Align::Fill);
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
        let system = format!("{}\n\n{}", zo_core::store::memory::system_with(agent::SYSTEM_PROMPT, &text), self.extra.borrow());
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
        let effort = sel.effort.clone();
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
                None => claude_code::run(&model, effort.as_deref(), &mut session, &text, &extra, &cancel, &mut emit),
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
                    Ok(Msg::Ev(Event::Usage(u))) => me.last_usage.set(Some(u)),
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
                        me.end_group();
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
pub(crate) fn nice_name(model: &str) -> String {
    // Claude models are named from the list that was read (or made from their id); anything else shows as it is
    if model.starts_with("claude-") || ["sonnet", "opus", "haiku", "fable"].contains(&model) {
        zo_core::ai::models::display_name(model)
    } else {
        model.to_string()
    }
}

/// What the model button says.
pub fn pill_text(sel: &Selection) -> String {
    match providers::find(&sel.provider) {
        Some(_) if !sel.model.is_empty() => nice_name(&sel.model),
        Some(p) if p.protocol == Protocol::ClaudeCode => "Claude".to_string(),
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
    // Can the model look at pictures (screenshots, images in a message)? Normally decided from the model's name.
    let vision_dd = gtk::DropDown::from_strings(&["Decide from the model's name (recommended)", "This model can see pictures", "This model cannot see pictures"]);
    vision_dd.set_selected(match sel.vision {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    });
    body.append(&crate::field("Pictures", &vision_dd));
    // Pin: the model then shows in the model menu, under its profile.
    let pin_check = gtk::CheckButton::with_label("Show this model in the model menu");
    pin_check.set_active(sel.profile.as_deref().and_then(|id| profiles::load().into_iter().find(|p| p.id == id)).is_some_and(|p| p.pinned.contains(&sel.model)));
    body.append(&pin_check);

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
            key.set_placeholder_text(Some(if profiles::get_key(&Selection::load(), p.id).is_some() { "Saved in the keyring — type to replace" } else { "Paste the key" }));
            model.set_text(if s.provider == p.id { &s.model } else { p.models.first().copied().unwrap_or("") });
            status.set_text(if p.protocol == Protocol::ClaudeCode {
                if claude_code::binary().is_some() { "Uses the Claude Code program you installed and signed in to yourself (claude auth login). ZO-AI never sees your login. Experimental. The full model list is read with an Anthropic API key (add one under the Anthropic provider or in a profile, then press Load the list); without one you get Claude Code's own names for the newest models." } else { "Claude Code is not installed. Install it with: npm install -g @anthropic-ai/claude-code, then sign in with: claude auth login" }
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
            let k = if typed.is_empty() { profiles::get_key(&Selection::load(), p.id) } else { Some(typed) };
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
        let vision_dd = vision_dd.clone();
        let pin_check = pin_check.clone();
        save.connect_clicked(move |_| {
            let mut sel = Selection::load();
            sel.vision = match vision_dd.selected() {
                1 => Some(true),
                2 => Some(false),
                _ => None,
            };
            let p = current(&provider_dd);
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
            // pin or unpin the model (a pin belongs to a profile; one is made from the provider's name when none is in use)
            if pin_check.is_active() && !sel.model.is_empty() {
                if let Ok(id) = profiles::ensure_active(&mut sel, p.name) {
                    sel.save();
                    profiles::pin(&id, &sel.model);
                }
            } else if let Some(id) = sel.profile.clone() {
                profiles::unpin(&id, &sel.model);
            }
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
    match p.id {
        // Claude: read from the Models API (or Claude Code's own names); the newest of each kind first, the rest under "More models"
        "anthropic" | "claude-code" => {
            let (main, _) = zo_core::ai::models::split_main(&zo_core::ai::models::claude_models());
            main.into_iter().map(|m| (m.id.clone(), m.name.clone(), Some(if m.note.is_empty() { m.id.clone() } else { m.note.clone() }))).collect()
        }
        _ => {
            let mut v: Vec<(String, String, Option<String>)> = p.models.iter().map(|m| (m.to_string(), m.to_string(), None)).collect();
            if !sel.model.is_empty() && sel.provider == p.id && !v.iter().any(|(id, _, _)| *id == sel.model) {
                v.insert(0, (sel.model.clone(), sel.model.clone(), None));
            }
            v
        }
    }
}

/// The older Claude models, under "More models".
fn more_choices() -> Vec<(String, String, Option<String>)> {
    let (_, more) = zo_core::ai::models::split_main(&zo_core::ai::models::claude_models());
    more.into_iter().map(|m| (m.id.clone(), m.name.clone(), Some(if m.note.is_empty() { m.id.clone() } else { m.note.clone() }))).collect()
}

/// The name shown for an effort level.
fn effort_name(level: &str) -> &'static str {
    match level {
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra high",
        "max" => "Max",
        _ => "Default",
    }
}

/// The model menu above the model button.
pub fn show_model_popover(ui: &Rc<Ui>, btn: &gtk::Button) {
    let sel = Selection::load();
    let pop = new_popover(btn);
    let body = vbox(0);
    // Models that were pinned (in the model settings) show here, grouped by their profile: one click switches profile and model.
    let all_profiles = profiles::load();
    let pinned_profiles: Vec<&profiles::Profile> = all_profiles.iter().filter(|p| !p.pinned.is_empty()).collect();
    let have_pins = !pinned_profiles.is_empty();
    for prof in &pinned_profiles {
        let provider_name = providers::find(&prof.provider).map(|x| x.name).unwrap_or("?");
        body.append(&lbl(&format!("{} · {}", prof.name, provider_name), "pop-head"));
        for model in &prof.pinned {
            let row = choice_row(&nice_name(model), None, sel.profile.as_deref() == Some(prof.id.as_str()) && sel.model == *model);
            let (ui2, pop2, prof2, model2) = (ui.clone(), pop.clone(), (*prof).clone(), model.clone());
            row.connect_clicked(move |_| {
                let mut s = Selection::load();
                s.apply_profile(&prof2);
                s.model = model2.clone();
                s.save();
                profiles::update_active(&s);
                ui2.model_label.set_text(&pill_text(&s));
                ui2.pv_model_label.set_text(&pill_text(&s));
                pop2.popdown();
            });
            body.append(&row);
        }
    }
    // what is in use now, when it is not one of the pinned ones
    let in_pins = pinned_profiles.iter().any(|p| sel.profile.as_deref() == Some(p.id.as_str()) && p.pinned.contains(&sel.model));
    if have_pins && !in_pins && !sel.model.is_empty() {
        body.append(&lbl("In use now", "pop-head"));
        body.append(&choice_row(&nice_name(&sel.model), providers::find(&sel.provider).map(|x| x.name), true));
    }
    match providers::find(&sel.provider) {
        Some(p) => {
            if !have_pins {
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
            }
            body.append(&separator());
            // Claude: how hard it thinks (only the levels this model has), and the older models; each opens in place under its row
            if p.protocol == Protocol::ClaudeCode {
                let levels = zo_core::ai::models::effort_levels(&sel.model);
                if !levels.is_empty() {
                    let current_effort = sel.effort.clone().unwrap_or_default();
                    let effort_row = choice_row(&format!("Effort · {}", effort_name(&current_effort)), Some("How hard it thinks before answering"), false);
                    let effort_list = vbox(0);
                    let effort_reveal = gtk::Revealer::new();
                    for value in std::iter::once(String::new()).chain(levels) {
                        let row = choice_row(effort_name(&value), None, value == current_effort);
                        let (ui2, pop2) = (ui.clone(), pop.clone());
                        row.connect_clicked(move |_| {
                            let mut s = Selection::load();
                            s.effort = Some(value.clone()).filter(|v| !v.is_empty());
                            s.save();
                            ui2.model_label.set_text(&pill_text(&s));
                            pop2.popdown();
                        });
                        effort_list.append(&row);
                    }
                    effort_reveal.set_child(Some(&effort_list));
                    {
                        let r = effort_reveal.clone();
                        effort_row.connect_clicked(move |_| r.set_reveal_child(!r.reveals_child()));
                    }
                    body.append(&effort_row);
                    body.append(&effort_reveal);
                }

                let older = more_choices();
                if !older.is_empty() {
                    let more_row = choice_row("More models", None, false);
                    let more_list = vbox(0);
                    let more_reveal = gtk::Revealer::new();
                    for (id, title, desc) in older {
                        let row = choice_row(&title, desc.as_deref(), sel.model == id);
                        let (ui2, pop2) = (ui.clone(), pop.clone());
                        row.connect_clicked(move |_| {
                            let mut s = Selection::load();
                            s.provider = "claude-code".to_string();
                            s.model = id.clone();
                            s.save();
                            ui2.model_label.set_text(&pill_text(&s));
                            ui2.pv_model_label.set_text(&pill_text(&s));
                            pop2.popdown();
                        });
                        more_list.append(&row);
                    }
                    more_reveal.set_child(Some(&more_list));
                    {
                        let r = more_reveal.clone();
                        more_row.connect_clicked(move |_| r.set_reveal_child(!r.reveals_child()));
                    }
                    body.append(&more_row);
                    body.append(&more_reveal);
                }
                body.append(&separator());
            }
        }
        None => body.append(&lbl("Choose a provider to begin", "pop-head")),
    }
    // The way to the provider settings only shows while there is nothing to choose from (no provider yet, or no models listed);
    // after that, Settings has it.
    let nothing_to_pick = providers::find(&sel.provider).is_none_or(|p| !have_pins && model_choices(p, &sel).is_empty());
    if nothing_to_pick {
        let more = choice_row("Provider, key and address…", None, false);
        let (ui2, pop2) = (ui.clone(), pop.clone());
        more.connect_clicked(move |_| {
            pop2.popdown();
            models_sheet(&ui2);
        });
        body.append(&more);
    }
    // the list scrolls instead of growing out of the window
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&body));
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_propagate_natural_height(true);
    scroll.set_min_content_width(320);
    scroll.set_max_content_height((ui.window.height() - 180).max(240));
    pop.set_child(Some(&scroll));
    pop.popup();
}

// ---------------------------------------------------------------- context

/// One row of the Context panel: a colour swatch, a name, the tokens and the share of the window.
fn context_row(class: &str, name: &str, tokens: u64, window: u64, dim: bool) -> gtk::Box {
    let r = hbox(10);
    r.add_css_class("ctx-row");
    let sw = vbox(0);
    sw.add_css_class("ctx-swatch");
    sw.add_css_class(class);
    sw.set_size_request(12, 12);
    sw.set_valign(gtk::Align::Center);
    r.append(&sw);
    let n = lbl(name, if dim { "ctx-name dim" } else { "ctx-name" });
    n.set_hexpand(true);
    n.set_xalign(0.0);
    r.append(&n);
    r.append(&lbl(&zo_core::ai::context::short(tokens), "ctx-num"));
    let pct = if window == 0 { 0.0 } else { tokens as f64 * 100.0 / window as f64 };
    let p = lbl(&format!("{pct:.1}%"), "ctx-pct");
    p.set_width_chars(6);
    p.set_xalign(1.0);
    r.append(&p);
    r
}

/// Fills the Context panel from numbers (the MCP ones may come later: `mcp` is None until the servers have answered).
fn fill_context(ui: &Rc<Ui>, body: &gtk::Box, pop: &Rc<dyn Fn()>, mcp: Option<(u64, u64, usize)>) {
    use zo_core::ai::context;
    while let Some(c) = body.first_child() {
        body.remove(&c);
    }
    let sel = Selection::load();
    let Some(p) = providers::find(&sel.provider) else {
        body.append(&lbl("Choose a provider first.", "pop-sub"));
        return;
    };
    if p.protocol == Protocol::ClaudeCode {
        body.append(&lbl("In Claude Code mode, Claude Code keeps this conversation and compacts it by itself, so ZO-AI has nothing to show here.", "pop-sub"));
        return;
    }
    let window = context::window_for(p, &sel.model);
    let history = ui.chat.history_snapshot();
    let system_prompt = context::tokens(agent::SYSTEM_PROMPT) + context::tokens(if zo_core::ai::providers::can_see(p, &sel.model) { agent::VISION_YES } else { agent::VISION_NO });
    let system_tools = context::specs_tokens(&zo_core::ai::wire::builtin_specs());
    let (mcp_tools, mcp_instr, mcp_count) = mcp.unwrap_or((0, 0, 0));
    let b = context::breakdown(window, context::history_tokens(&history), system_tools, mcp_tools, system_prompt, mcp_instr, context::tokens(&ui.chat.extra_text()));

    let pct = if window == 0 { 0.0 } else { b.used as f64 * 100.0 / window as f64 };
    let top = hbox(8);
    top.append(&lbl(&format!("{pct:.0}% used"), "ctx-big"));
    let of = lbl(&format!("≈ {} / {}", context::short(b.used), context::short(window)), "ctx-of");
    of.set_hexpand(true);
    of.set_xalign(1.0);
    top.append(&of);
    body.append(&top);

    // the bar: one coloured piece for each part, then the buffer kept for the summary and the reply, then what is free
    let bar = hbox(0);
    bar.add_css_class("ctx-bar");
    let width = 400.0;
    for (i, (_, t)) in b.items.iter().enumerate() {
        let w = ((*t as f64 / window.max(1) as f64) * width).round() as i32;
        if w > 0 {
            let seg = vbox(0);
            seg.add_css_class(&format!("ctx-c{i}"));
            seg.set_size_request(w.max(2), 8);
            bar.append(&seg);
        }
    }
    let free_w = ((b.free as f64 / window.max(1) as f64) * width).round() as i32;
    if free_w > 0 {
        let seg = vbox(0);
        seg.add_css_class("ctx-free");
        seg.set_size_request(free_w, 8);
        bar.append(&seg);
    }
    let buf_w = ((b.buffer as f64 / window.max(1) as f64) * width).round() as i32;
    if buf_w > 0 {
        let seg = vbox(0);
        seg.add_css_class("ctx-buffer");
        seg.set_size_request(buf_w, 8);
        bar.append(&seg);
    }
    body.append(&bar);

    for (i, (name, t)) in b.items.iter().enumerate() {
        let shown = if *name == "MCP tools" && mcp.is_some() { format!("MCP tools ({mcp_count})") } else { name.to_string() };
        let label = if mcp.is_none() && (*name == "MCP tools" || *name == "MCP server instructions") { format!("{name} …") } else { shown };
        body.append(&context_row(&format!("ctx-c{i}"), &label, *t, window, false));
    }
    body.append(&context_row("ctx-buffer", "Auto-compact buffer", b.buffer, window, true));
    body.append(&context_row("ctx-free", "Free space", b.free, window, true));

    if let Some(u) = ui.chat.usage() {
        let cached = if u.cached > 0 { format!(" ({} from the provider's cache)", context::short(u.cached)) } else { String::new() };
        let l = lbl(&format!("Last request, as the provider counted it: {} in{cached}, {} out.", context::short(u.total_input()), context::short(u.output)), "ctx-note");
        l.set_wrap(true);
        l.set_xalign(0.0);
        body.append(&l);
    }
    let note = lbl("Sizes are estimated from the length of the text; the provider's count is exact.", "ctx-note");
    note.set_wrap(true);
    note.set_xalign(0.0);
    body.append(&note);

    let foot = hbox(8);
    let until = lbl(&format!("{} until auto-compact", context::short(b.until_compact)), "ctx-note");
    until.set_hexpand(true);
    until.set_xalign(0.0);
    foot.append(&until);
    let compact = secondary("Compact session");
    {
        let (ui2, pop2) = (ui.clone(), pop.clone());
        let status = until.clone();
        compact.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            status.set_text("Summarizing…");
            let (ui3, pop3, btn3, status3) = (ui2.clone(), pop2.clone(), btn.clone(), status.clone());
            ui2.chat.compact_now(Rc::new(move |r| match r {
                Ok(()) => pop3(),
                Err(e) => {
                    btn3.set_sensitive(true);
                    status3.set_text(&e);
                    let _ = &ui3;
                }
            }));
        });
    }
    foot.append(&compact);
    body.append(&foot);
}

/// The Context panel in the top bar: where the model's window goes, and how long until the chat is compacted.
pub fn show_context_popover(ui: &Rc<Ui>, _btn: &gtk::Button) {
    // already pinned at the side: show that instead
    if ui.dock.context_pinned() {
        ui.dock.show("context");
        ui.dock.refresh_context_now();
        return;
    }
    // an in-app panel (not a popover, which could stick out of the window)
    let (win, body, foot) = sheet(ui, "Context", 480);
    body.add_css_class("ctx-panel");
    let pin = gtk::Button::with_label("Pin to the side");
    pin.set_tooltip_text(Some("Keep this panel open next to the chat. Drag its edge to change its width."));
    {
        let (ui2, w) = (ui.clone(), win.clone());
        pin.connect_clicked(move |_| {
            w.close();
            pin_context(&ui2);
        });
    }
    foot.append(&pin);
    let w = win.clone();
    let pop: Rc<dyn Fn()> = Rc::new(move || w.close());
    load_context(ui, &body, &pop);
    win.present();
}

/// Pins the Context panel to the right side of the window; it follows the chat from then on.
pub fn pin_context(ui: &Rc<Ui>) {
    let dock = &ui.dock;
    dock.pin_context(true);
    let (ui2, body) = (ui.clone(), dock.context_body.clone());
    let noop: Rc<dyn Fn()> = Rc::new(|| {});
    let unpin = {
        let (ui3, body2) = (ui.clone(), body.clone());
        let _ = &body2;
        move || ui3.dock.pin_context(false)
    };
    let refresh: Rc<dyn Fn()> = {
        let (ui3, body3, noop3) = (ui2.clone(), body.clone(), noop.clone());
        Rc::new(move || {
            load_context(&ui3, &body3, &noop3);
            // the unpin button goes at the end of the panel
            let b = gtk::Button::with_label("Unpin");
            b.set_halign(gtk::Align::End);
            let u = unpin.clone();
            b.connect_clicked(move |_| u());
            body3.append(&b);
        })
    };
    *dock.refresh_context.borrow_mut() = Some(refresh.clone());
    refresh();
}

/// Fills the Context panel now, then again when the MCP servers have said what their tools cost (which can take a moment).
pub fn load_context(ui: &Rc<Ui>, body: &gtk::Box, pop: &Rc<dyn Fn()>) {
    fill_context(ui, body, pop, None);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ext = zo_core::mcp_client::External::start();
        let specs: Vec<zo_core::ai::wire::ToolSpec> = ext.tools().iter().map(|t| zo_core::ai::wire::ToolSpec { name: t.public_name(), description: t.description.clone(), schema: t.schema.clone() }).collect();
        let instr: u64 = ext.instructions().iter().map(|(s, t)| zo_core::ai::context::tokens(&format!("[{s}] {t}"))).sum();
        let _ = tx.send((zo_core::ai::context::specs_tokens(&specs), instr, specs.len()));
    });
    let (ui2, body2, pop2) = (ui.clone(), body.clone(), pop.clone());
    glib::timeout_add_local(Duration::from_millis(250), move || match rx.try_recv() {
        Ok(m) => {
            if body2.root().is_some() {
                fill_context(&ui2, &body2, &pop2, Some(m));
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(_) => glib::ControlFlow::Break,
    });
}

// ---------------------------------------------------------------- profiles

/// A small panel that asks for a name (making a profile, renaming one).
pub(crate) fn name_sheet(ui: &Rc<Ui>, title: &str, initial: &str, ok_text: &str, on_ok: impl Fn(String) -> Result<(), String> + 'static) {
    let (win, body, foot) = sheet(ui, title, 420);
    let name = gtk::Entry::new();
    name.set_text(initial);
    name.set_placeholder_text(Some("Work, Home, Free models…"));
    name.add_css_class("field-input");
    body.append(&crate::field("Name", &name));
    let status = lbl("", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);
    let cancel = secondary("Cancel");
    let ok = primary(ok_text);
    foot.append(&cancel);
    foot.append(&ok);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    let go = {
        let (w, name, status) = (win.clone(), name.clone(), status.clone());
        Rc::new(move || match on_ok(name.text().to_string()) {
            Ok(()) => w.close(),
            Err(e) => status.set_text(&e),
        })
    };
    {
        let go = go.clone();
        ok.connect_clicked(move |_| go());
    }
    name.connect_activate(move |_| go());
    win.present();
    name.grab_focus();
}

pub(crate) fn describe(p: &profiles::Profile) -> String {
    let provider = providers::find(&p.provider).map(|x| x.name).unwrap_or("?");
    format!("{provider} · {}", if p.model.is_empty() { "no model yet".to_string() } else { nice_name(&p.model) })
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
