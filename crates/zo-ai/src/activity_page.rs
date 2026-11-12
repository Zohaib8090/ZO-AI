//! The Activity page: what is running right now (with Stop), what is connected, and the services that work in the background.
//!
//! It reads `zo_core::activity` (the same registry the tray and the phone and web apps show). While the page is open it looks again
//! every two seconds and rebuilds only when something is different; the running times tick every second in place.

use crate::{hbox, icon, lbl, vbox, Ui};
use gtk::glib;
use gtk4 as gtk;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use zo_core::activity::{self, Kind};
use zo_core::store::chats;

pub struct Page {
    /// The scrolled content of the page.
    pub list: gtk::Box,
    /// What the page shows now, as text (so an unchanged page is not rebuilt).
    sig: RefCell<String>,
    /// The running-time labels: (label, started_ms), updated every second.
    times: RefCell<Vec<(gtk::Label, u64)>>,
}

impl Page {
    pub fn new() -> Rc<Page> {
        let list = vbox(8);
        list.set_margin_start(44);
        list.set_margin_end(44);
        list.set_margin_bottom(30);
        Rc::new(Page { list, sig: RefCell::new(String::new()), times: RefCell::new(Vec::new()) })
    }
}

thread_local! {
    /// What the last press of Stop said, by item id (shown in the row until the item is gone).
    static STOPPED: RefCell<HashMap<u64, String>> = RefCell::new(HashMap::new());
    /// The last "Sync now" result.
    static SYNC_NOW: RefCell<String> = const { RefCell::new(String::new()) };
    /// Labels whose text holds a "5 s ago" stamp: (label, text with the stamp markers), refreshed in place every second.
    static STAMPED: RefCell<Vec<(gtk::Label, String)>> = const { RefCell::new(Vec::new()) };
}

/// A moment inside a text: kept as the time itself (so the page signature does not change every second) and turned into "5 s ago"
/// when the label is shown or ticks.
fn stamp(at: u64) -> String {
    format!("\u{1}{at}\u{2}")
}

/// Replaces every stamp marker by how long ago it was.
fn fill(tpl: &str) -> String {
    let now = activity::now_ms();
    let mut out = String::new();
    let mut rest = tpl;
    while let Some(a) = rest.find('\u{1}') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('\u{2}') {
            Some(b) => {
                let at: u64 = after[..b].parse().unwrap_or(0);
                out.push_str(&activity::ago(now.saturating_sub(at)));
                rest = &after[b + 1..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// "12 s", "3 min 5 s", "1 h 2 min".
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{} h {} min", s / 3600, (s % 3600) / 60)
    }
}

fn kind_icon(k: Kind) -> &'static str {
    match k {
        Kind::Answer => "chat-message-new-symbolic",
        Kind::ClaudeCode => "utilities-terminal-symbolic",
        Kind::Scheduled => "alarm-symbolic",
        Kind::MemoryJob => "preferences-other-symbolic",
        Kind::Command => "system-run-symbolic",
        Kind::Terminal => "utilities-terminal-symbolic",
        Kind::Sync => "emblem-synchronizing-symbolic",
        Kind::Client => "network-workgroup-symbolic",
        Kind::Other => "emblem-system-symbolic",
    }
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Answer => "Chat",
        Kind::ClaudeCode => "Claude Code",
        Kind::Terminal => "Terminal",
        Kind::Scheduled => "Scheduled task",
        Kind::MemoryJob => "Memory",
        Kind::Sync => "Sync",
        Kind::Client => "Client",
        Kind::Command => "Command",
        Kind::Other => "Other",
    }
}

fn source_word(s: &str) -> &str {
    match s {
        "desktop" => "this app",
        "phone" => "the phone",
        "web" => "the web app",
        "web-link" => "a web session",
        "cli" => "the command line",
        "scheduler" => "the scheduler",
        "background" => "in the background",
        o => o,
    }
}

/// Everything the page shows, read once, so that "did anything change" is one comparison.
#[derive(Debug, Default)]
struct Snapshot {
    running: Vec<(u64, Kind, String, String, String, Option<String>, u64, bool, String)>,
    terminals: Vec<(u64, String)>,
    phone: String,
    web: String,
    mcp: String,
    sync: Vec<String>,
    sync_on: bool,
    services: Vec<(String, String)>,
}

fn seen_line(source: &[&str], seen: &[(String, u64)]) -> String {
    let now = activity::now_ms();
    match seen.iter().filter(|(s, _)| source.contains(&s.as_str())).map(|(_, at)| *at).max() {
        // a phone or a browser asks every few seconds while it is open
        Some(at) if now.saturating_sub(at) < 30_000 => format!("Connected, last request {}", stamp(at)),
        Some(at) => format!("Last seen {}", stamp(at)),
        None => "Not connected yet".into(),
    }
}

fn snapshot() -> Snapshot {
    let all = activity::list();
    let seen = activity::clients_seen();
    let mut s = Snapshot::default();
    for i in &all {
        match i.kind {
            Kind::Sync | Kind::Client => {}
            Kind::Terminal => s.terminals.push((i.id, i.title.clone())),
            _ => {
                let msg = STOPPED.with(|m| m.borrow().get(&i.id).cloned()).unwrap_or_default();
                s.running.push((i.id, i.kind, i.title.clone(), i.detail.clone(), i.source.clone(), i.chat_id.clone(), i.started_ms, i.can_stop(), msg));
            }
        }
    }
    // a "Stop" message is forgotten once its item is gone
    STOPPED.with(|m| m.borrow_mut().retain(|id, _| all.iter().any(|i| i.id == *id)));
    s.phone = seen_line(&["phone"], &seen);
    s.web = seen_line(&["web", "cli"], &seen);

    let servers = zo_core::mcp_client::load_servers();
    s.mcp = if servers.is_empty() {
        "None set up. Add one in Settings, Connect other apps.".into()
    } else {
        let on: Vec<String> = servers.iter().filter(|x| x.enabled && !x.held).map(|x| x.name.clone()).collect();
        format!("{} set up ({} on): {}", servers.len(), on.len(), servers.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", "))
    };

    let cfg = zo_core::sync::config();
    let st = zo_core::sync::status();
    s.sync_on = cfg.enabled;
    let sync_state = if !cfg.enabled {
        "Sign in under Settings, Sync, to keep chats and memory on your other devices.".to_string()
    } else if st.running {
        "Syncing now…".to_string()
    } else if !st.error.is_empty() {
        format!("Problem: {}", st.error)
    } else if st.last_ok > 0 {
        format!("Up to date, last synced {}", stamp(st.last_ok))
    } else {
        "Waiting for the first sync".to_string()
    };
    s.sync = if cfg.enabled {
        vec![if cfg.email.is_empty() { "Sync is on".to_string() } else { format!("Signed in as {}", cfg.email) }, sync_state, SYNC_NOW.with(|n| n.borrow().clone())]
    } else {
        vec![format!("Not signed in. {}", sync_state), SYNC_NOW.with(|n| n.borrow().clone())]
    };

    let sock = zo_core::service::socket_path();
    s.services.push(("ZO-AI service".into(), if sock.exists() { "Listening for the phone, the web app and the command line".into() } else { "Not running".into() }));
    let tasks = zo_core::store::schedules::load();
    let on: Vec<_> = tasks.iter().filter(|t| t.enabled).collect();
    let next = on.iter().map(|t| t.next_run).min();
    s.services.push((
        "Scheduler".into(),
        match next {
            Some(n) => format!("{} task{} on, next {}. Looks every 20 seconds.", on.len(), if on.len() == 1 { "" } else { "s" }, zo_core::store::schedules::format_when(n)),
            None => "No task is on. Looks every 20 seconds.".into(),
        },
    ));
    let mem = zo_core::store::memory::load();
    let last = mem.items.iter().map(|i| i.updated.max(i.created)).max().unwrap_or(0);
    let mut m = if mem.enabled { format!("On, reads a chat after every {} of your messages", mem.every) } else { "Off".to_string() };
    if zo_core::ai::memory_agent::is_running() {
        m += ". Reading a chat now";
    } else if last > 0 {
        m += &format!(". Last note saved {}", stamp(last));
    }
    s.services.push(("Memory agent".into(), m));
    s.services.push(("Sync loop".into(), if cfg.enabled { "On, every 30 seconds".into() } else { "Off".into() }));
    s.services.push((
        "Tray icon".into(),
        if crate::tray::present() { "In the panel".into() } else { "No panel that shows tray icons was found".into() },
    ));
    s
}

fn section(title: &str) -> (gtk::Box, gtk::Box) {
    let outer = vbox(8);
    outer.set_margin_bottom(26);
    outer.append(&lbl(title, "settings-head"));
    let card = vbox(0);
    card.add_css_class("settings-card");
    outer.append(&card);
    (outer, card)
}

/// A row: an icon, a title with a line under it, and room for buttons on the right.
fn line(ic: &str, title: &str, sub: &str) -> (gtk::Box, gtk::Box) {
    let r = hbox(14);
    r.add_css_class("settings-row");
    let i = icon(ic, 18);
    i.add_css_class("pc-icon");
    i.set_valign(gtk::Align::Center);
    r.append(&i);
    let col = vbox(3);
    col.set_hexpand(true);
    col.set_valign(gtk::Align::Center);
    let t = lbl(title, "pc-name");
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    col.append(&t);
    if !sub.is_empty() {
        let s = lbl(&fill(sub), "pc-meta");
        if sub.contains('\u{1}') {
            STAMPED.with(|v| v.borrow_mut().push((s.clone(), sub.to_string())));
        }
        s.set_wrap(true);
        s.set_max_width_chars(70);
        col.append(&s);
    }
    r.append(&col);
    let right = hbox(8);
    right.set_valign(gtk::Align::Center);
    r.append(&right);
    (r, right)
}

fn small_button(text: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("page-btn");
    b
}

/// Builds the page again from a snapshot.
fn rebuild(ui: &Rc<Ui>, s: &Snapshot) {
    let page = &ui.activity;
    STAMPED.with(|v| v.borrow_mut().clear());
    while let Some(c) = page.list.first_child() {
        page.list.remove(&c);
    }
    let mut times = Vec::new();

    // ---- running now ----
    let (sec, card) = section("Running now");
    if s.running.is_empty() {
        // no card around the empty message: it is a box of its own
        sec.remove(&card);
        let empty = vbox(10);
        empty.add_css_class("empty-box");
        empty.set_margin_top(18);
        empty.set_margin_bottom(22);
        let t = lbl("Nothing is running", "empty-title");
        t.set_halign(gtk::Align::Center);
        let d = lbl("Chats being answered, scheduled tasks, the memory agent and long commands show up here while they work.", "empty-text");
        d.set_halign(gtk::Align::Center);
        d.set_wrap(true);
        d.set_justify(gtk::Justification::Center);
        d.set_max_width_chars(60);
        empty.append(&t);
        empty.append(&d);
        sec.append(&empty);
    }
    for (id, kind, title, detail, source, chat_id, started, can_stop, msg) in &s.running {
        let mut sub = format!("{} from {}", kind_word(*kind), source_word(source));
        if !detail.is_empty() {
            sub += &format!(" · {detail}");
        }
        let (row, right) = line(kind_icon(*kind), title, &sub);
        let t = lbl(&clock(activity::now_ms().saturating_sub(*started)), "pc-meta");
        t.set_valign(gtk::Align::Center);
        right.append(&t);
        times.push((t, *started));
        if !msg.is_empty() {
            right.append(&lbl(msg, "pc-meta"));
        }
        if let Some(cid) = chat_id.clone() {
            let open = small_button("Open chat");
            let ui2 = ui.clone();
            open.connect_clicked(move |_| match chats::load().into_iter().find(|c| c.id == cid) {
                Some(c) => crate::open_chat(&ui2, c),
                None => {
                    ui2.stack.set_visible_child_name("chat");
                    crate::set_nav(&ui2, "new");
                }
            });
            right.append(&open);
        }
        if *can_stop {
            let stop = small_button("Stop");
            stop.add_css_class("danger");
            let (id, ui2) = (*id, ui.clone());
            stop.connect_clicked(move |b| {
                let ok = activity::stop(id);
                STOPPED.with(|m| m.borrow_mut().insert(id, if ok { "Stop sent".into() } else { "Could not stop".into() }));
                b.set_sensitive(false);
                b.set_label(if ok { "Stopping…" } else { "Could not stop" });
                // look again soon: a stopped answer is gone within a moment
                let ui3 = ui2.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(700), move || refresh(&ui3, true));
            });
            right.append(&stop);
        } else {
            right.append(&lbl("Cannot be stopped", "pc-meta"));
        }
        card.append(&row);
    }
    page.list.append(&sec);

    // ---- connected ----
    let (sec, card) = section("Connected");
    let (row, _) = line("phone-symbolic", "Phone link", &s.phone);
    card.append(&row);
    let (row, _) = line("network-workgroup-symbolic", "Web app and command line", &s.web);
    card.append(&row);
    let (row, _) = line("application-x-addon-symbolic", "MCP servers", &s.mcp);
    card.append(&row);
    {
        let sub = s.sync.iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join("\n");
        let (row, right) = line("emblem-synchronizing-symbolic", "Sync", &sub);
        if s.sync_on {
            let now = small_button("Sync now");
            let ui2 = ui.clone();
            now.connect_clicked(move |b| {
                b.set_sensitive(false);
                SYNC_NOW.with(|n| *n.borrow_mut() = "Syncing…".into());
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(zo_core::sync::sync_now());
                });
                let ui3 = ui2.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(200), move || match rx.try_recv() {
                    Ok(r) => {
                        let text = match r {
                            Ok((pulled, pushed, deleted)) => format!("Synced: {pulled} in, {pushed} out, {deleted} removed"),
                            Err(e) => format!("Sync failed: {e}"),
                        };
                        SYNC_NOW.with(|n| *n.borrow_mut() = text);
                        refresh(&ui3, true);
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => glib::ControlFlow::Break,
                });
                refresh(&ui2, true);
            });
            right.append(&now);
        }
        card.append(&row);
    }
    if s.terminals.is_empty() {
        let (row, _) = line("utilities-terminal-symbolic", "Terminals", "None open");
        card.append(&row);
    }
    for (id, name) in &s.terminals {
        let (row, right) = line("utilities-terminal-symbolic", &format!("Terminal: {name}"), "Open in the side panel, where you can type in it or watch what ZO-AI runs");
        let open = small_button("Open");
        let ui2 = ui.clone();
        open.connect_clicked(move |_| {
            ui2.dock.show("terminal");
        });
        right.append(&open);
        let close = small_button("Close");
        close.add_css_class("danger");
        let (id, ui2) = (*id, ui.clone());
        close.connect_clicked(move |_| {
            activity::stop(id);
            refresh(&ui2, true);
        });
        right.append(&close);
        card.append(&row);
    }
    page.list.append(&sec);

    // ---- background services ----
    let (sec, card) = section("Background services");
    for (name, text) in &s.services {
        let (row, _) = line("emblem-system-symbolic", name, text);
        card.append(&row);
    }
    page.list.append(&sec);

    *page.times.borrow_mut() = times;
}

/// Looks at the state; rebuilds the page only when it is different (or when `force`).
pub fn refresh(ui: &Rc<Ui>, force: bool) {
    let s = snapshot();
    let sig = format!("{s:?}");
    if !force && *ui.activity.sig.borrow() == sig {
        return;
    }
    *ui.activity.sig.borrow_mut() = sig;
    rebuild(ui, &s);
}

/// Called when the page is opened, and starts the clock that keeps it fresh while it is visible.
pub fn open(ui: &Rc<Ui>) {
    ui.stack.set_visible_child_name("activity");
    crate::set_nav(ui, "activity");
    refresh(ui, true);
}

/// Once per window: every second the running times tick; every other second the page looks for changes. Nothing happens while the
/// page is not the one on show.
pub fn watch(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    let tick = std::cell::Cell::new(0u32);
    glib::timeout_add_seconds_local(1, move || {
        let Some(ui) = weak.upgrade() else { return glib::ControlFlow::Break };
        if !crate::main_window_open() {
            return glib::ControlFlow::Break;
        }
        if ui.stack.visible_child_name().as_deref() != Some("activity") {
            return glib::ControlFlow::Continue;
        }
        tick.set(tick.get() + 1);
        if tick.get() % 2 == 0 {
            refresh(&ui, false);
        }
        let now = activity::now_ms();
        for (l, started) in ui.activity.times.borrow().iter() {
            l.set_text(&clock(now.saturating_sub(*started)));
        }
        STAMPED.with(|v| {
            for (l, tpl) in v.borrow().iter() {
                l.set_text(&fill(tpl));
            }
        });
        glib::ControlFlow::Continue
    });
}
