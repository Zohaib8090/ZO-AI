//! The tray icon: a StatusNotifierItem (what KDE Plasma and most other panels use) with a dbusmenu menu.
//!
//! It is written with GIO's D-Bus, so it needs no extra library, runs on the GTK main loop (no thread, nothing to lock against the
//! window) and watches for `org.kde.StatusNotifierWatcher` instead of polling: when a panel that can show tray icons is there (now,
//! or later, or again after the panel restarts) the icon registers itself; when there is none, nothing happens and nothing is
//! retried.
//!
//! The menu: Open ZO-AI, what is running now (a submenu with Stop for everything that can be stopped), Activity..., Web control (on/off), Quit.
//! A left click on the icon shows or hides the window. The tooltip says how many things are running, and the icon asks for
//! attention while an approval is waiting.

use gtk::gdk::prelude::TextureExt;
use glib::{Variant, VariantTy};
use glib::variant::ToVariant;
use gtk4 as gtk;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use zo_core::activity::{self, Kind};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const MENU_IFACE: &str = "com.canonical.dbusmenu";

const SNI_XML: &str = r#"<node>
  <interface name="org.kde.StatusNotifierItem">
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="WindowId" type="u" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="OverlayIconName" type="s" access="read"/>
    <property name="OverlayIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionIconName" type="s" access="read"/>
    <property name="AttentionIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionMovieName" type="s" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <method name="ContextMenu"><arg type="i" name="x" direction="in"/><arg type="i" name="y" direction="in"/></method>
    <method name="Activate"><arg type="i" name="x" direction="in"/><arg type="i" name="y" direction="in"/></method>
    <method name="SecondaryActivate"><arg type="i" name="x" direction="in"/><arg type="i" name="y" direction="in"/></method>
    <method name="Scroll"><arg type="i" name="delta" direction="in"/><arg type="s" name="orientation" direction="in"/></method>
    <signal name="NewTitle"/>
    <signal name="NewIcon"/>
    <signal name="NewAttentionIcon"/>
    <signal name="NewOverlayIcon"/>
    <signal name="NewToolTip"/>
    <signal name="NewStatus"><arg type="s" name="status"/></signal>
  </interface>
</node>"#;

const MENU_XML: &str = r#"<node>
  <interface name="com.canonical.dbusmenu">
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
    <method name="GetLayout">
      <arg type="i" name="parentId" direction="in"/>
      <arg type="i" name="recursionDepth" direction="in"/>
      <arg type="as" name="propertyNames" direction="in"/>
      <arg type="u" name="revision" direction="out"/>
      <arg type="(ia{sv}av)" name="layout" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg type="ai" name="ids" direction="in"/>
      <arg type="as" name="propertyNames" direction="in"/>
      <arg type="a(ia{sv})" name="properties" direction="out"/>
    </method>
    <method name="GetProperty">
      <arg type="i" name="id" direction="in"/>
      <arg type="s" name="name" direction="in"/>
      <arg type="v" name="value" direction="out"/>
    </method>
    <method name="Event">
      <arg type="i" name="id" direction="in"/>
      <arg type="s" name="eventId" direction="in"/>
      <arg type="v" name="data" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
    <method name="EventGroup">
      <arg type="a(isvu)" name="events" direction="in"/>
      <arg type="ai" name="idErrors" direction="out"/>
    </method>
    <method name="AboutToShow">
      <arg type="i" name="id" direction="in"/>
      <arg type="b" name="needUpdate" direction="out"/>
    </method>
    <method name="AboutToShowGroup">
      <arg type="ai" name="ids" direction="in"/>
      <arg type="ai" name="updatesNeeded" direction="out"/>
      <arg type="ai" name="idErrors" direction="out"/>
    </method>
    <signal name="ItemsPropertiesUpdated"><arg type="a(ia{sv})" name="updatedProps"/><arg type="a(ias)" name="removedProps"/></signal>
    <signal name="LayoutUpdated"><arg type="u" name="revision"/><arg type="i" name="parent"/></signal>
    <signal name="ItemActivationRequested"><arg type="i" name="id"/><arg type="u" name="timestamp"/></signal>
  </interface>
</node>"#;

// ids of the fixed menu entries
const ID_ROOT: i32 = 0;
const ID_TITLE: i32 = 1;
const ID_OPEN: i32 = 2;
const ID_SEP1: i32 = 3;
const ID_RUNNING: i32 = 4;
const ID_NOTHING: i32 = 5;
const ID_APPROVAL: i32 = 6;
const ID_SEP2: i32 = 7;
const ID_ACTIVITY: i32 = 8;
const ID_SEP3: i32 = 9;
const ID_QUIT: i32 = 10;
const ID_WEB: i32 = 11;
/// ids of the lines that come and go start here
const FIRST_DYNAMIC: i32 = 100;

#[derive(Clone)]
struct Node {
    id: i32,
    label: String,
    sep: bool,
    enabled: bool,
    kids: Vec<Node>,
}

impl Node {
    fn item(id: i32, label: &str, enabled: bool) -> Node {
        Node { id, label: label.to_string(), sep: false, enabled, kids: Vec::new() }
    }
    fn sep(id: i32) -> Node {
        Node { id, label: String::new(), sep: true, enabled: true, kids: Vec::new() }
    }
    fn find(&self, id: i32) -> Option<&Node> {
        if self.id == id {
            return Some(self);
        }
        self.kids.iter().find_map(|k| k.find(id))
    }
}

struct State {
    conn: Option<gio::DBusConnection>,
    name_ok: bool,
    watcher: bool,
    registered: bool,
    /// a RegisterStatusNotifierItem call is in flight
    registering: bool,
    /// the watcher says a tray (a host that draws the icons) is there
    host: bool,
    rev: u32,
    root: Node,
    /// the layout as text, to see whether anything changed (ids of the changing lines left out)
    sig: String,
    /// menu id of a Stop entry -> what it stops
    stops: HashMap<i32, u64>,
    next_id: i32,
    status: &'static str,
    tooltip: (String, String),
    pixmap: Option<Variant>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State {
        conn: None,
        name_ok: false,
        watcher: false,
        registered: false,
        registering: false,
        host: false,
        rev: 1,
        root: Node { id: ID_ROOT, label: String::new(), sep: false, enabled: true, kids: Vec::new() },
        sig: String::new(),
        stops: HashMap::new(),
        next_id: FIRST_DYNAMIC,
        status: "Active",
        tooltip: (String::new(), String::new()),
        pixmap: None,
    });
}

/// Is the icon in a panel right now? (The watcher took it, and a tray that draws the icons is there: a watcher alone, as the
/// Plasma shell has without a system tray applet, is not enough.)
pub fn present() -> bool {
    ST.with(|s| {
        let st = s.borrow();
        st.registered && st.host
    })
}

// ---------------------------------------------------------------- the settings the tray owns

fn settings_path() -> std::path::PathBuf {
    zo_core::audit::config_dir().join("tray.json")
}

/// What the person chose for "keep running in the tray when the window is closed"; None: never chosen.
pub fn keep_setting() -> Option<bool> {
    let v: serde_json::Value = std::fs::read_to_string(settings_path()).ok().and_then(|s| serde_json::from_str(&s).ok())?;
    v.get("keep").and_then(|k| k.as_bool())
}

pub fn set_keep(on: bool) {
    let p = settings_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
    let _ = std::fs::write(p, serde_json::json!({"keep": on}).to_string());
}

/// Closing the window keeps ZO-AI running: only when there is a tray icon to get it back from (never an invisible app), and the
/// person has not turned it off (default: on).
pub fn keep_running() -> bool {
    present() && keep_setting().unwrap_or(true)
}

fn autostart_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("autostart").join("zo-ai-background.desktop")
}

/// The autostart entry the package installs for every user, when there is one.
fn system_autostart() -> Option<std::path::PathBuf> {
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let dirs = if dirs.is_empty() { "/etc/xdg".to_string() } else { dirs };
    dirs.split(':').filter(|d| !d.is_empty()).map(|d| std::path::Path::new(d).join("autostart").join("zo-ai-background.desktop")).find(|p| p.is_file())
}

/// On when the person's own entry exists and is not switched off, or (without one) the package's entry for all users exists.
pub fn autostart_enabled() -> bool {
    match std::fs::read_to_string(autostart_path()) {
        Ok(t) => !t.lines().any(|l| l.trim().eq_ignore_ascii_case("Hidden=true")),
        Err(_) => system_autostart().is_some(),
    }
}

/// "Start with the computer": the autostart entry that starts ZO-AI without a window. Off writes an entry with `Hidden=true` when
/// the package's entry for all users exists (the only way to override it); otherwise the entry is removed.
pub fn set_autostart(on: bool) -> Result<(), String> {
    let p = autostart_path();
    if on {
        std::fs::create_dir_all(p.parent().unwrap_or(&p)).map_err(|e| e.to_string())?;
        return std::fs::write(&p, include_str!("../data/zo-ai-background.desktop")).map_err(|e| e.to_string());
    }
    if system_autostart().is_some() {
        std::fs::create_dir_all(p.parent().unwrap_or(&p)).map_err(|e| e.to_string())?;
        let off = format!("{}Hidden=true\n", include_str!("../data/zo-ai-background.desktop").trim_end().to_string() + "\n");
        return std::fs::write(&p, off).map_err(|e| e.to_string());
    }
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------- the menu

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Answer => "Chat",
        Kind::ClaudeCode => "Claude Code",
        Kind::Terminal => "Terminal",
        Kind::Scheduled => "Task",
        Kind::MemoryJob => "Memory",
        Kind::Sync => "Sync",
        Kind::Client => "Client",
        Kind::Command => "Command",
        Kind::Other => "Other",
    }
}

/// "under a minute", "2 min", "1 h 5 min".
pub fn elapsed(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        "under a minute".into()
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else {
        format!("{} h {} min", s / 3600, (s % 3600) / 60)
    }
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>())
    }
}

/// What the tray lists as running: everything the person can see work, not the sync's always-on line or the connected clients.
pub fn running_items() -> Vec<activity::Item> {
    activity::list().into_iter().filter(|i| !matches!(i.kind, Kind::Sync | Kind::Client)).collect()
}

/// The line for one running thing: "Chat: Plan the trip · 2 min".
pub fn line_for(i: &activity::Item, now: u64) -> String {
    let mut l = format!("{}: {}", kind_word(i.kind), short(&i.title, 60));
    if i.started_ms > 0 {
        l += &format!(" · {}", elapsed(now.saturating_sub(i.started_ms)));
    }
    l
}

fn menu_label(s: &str) -> String {
    // "_" marks a keyboard shortcut in a dbusmenu label
    s.replace('_', "__")
}

fn build(st: &mut State, items: &[activity::Item], waiting: bool) -> Node {
    let now = activity::now_ms();
    let mut kids = vec![Node::item(ID_TITLE, "ZO-AI", false), Node::item(ID_OPEN, "Open ZO-AI", true), Node::sep(ID_SEP1)];
    kids.push(Node::item(ID_RUNNING, "Running now", false));
    if items.is_empty() {
        kids.push(Node::item(ID_NOTHING, "Nothing is running", false));
    }
    for i in items {
        let id = st.next_id;
        st.next_id += 1;
        let mut n = Node::item(id, &menu_label(&line_for(i, now)), true);
        if i.can_stop() {
            let sid = st.next_id;
            st.next_id += 1;
            st.stops.insert(sid, i.id);
            n.kids.push(Node::item(sid, "Stop", true));
        } else {
            n.enabled = false;
        }
        kids.push(n);
    }
    if waiting {
        kids.push(Node::item(ID_APPROVAL, "Waiting for your approval…", true));
    }
    kids.push(Node::sep(ID_SEP2));
    kids.push(Node::item(ID_ACTIVITY, "Activity…", true));
    // "Control this computer from the web": a click turns it on or off (only for a signed-in computer)
    if zo_core::login::signed_in_quick().is_some() {
        kids.push(Node::item(ID_WEB, if zo_core::link::config().enabled { "Web control: on" } else { "Web control: off" }, true));
    } else {
        kids.push(Node::item(ID_WEB, "Web control: off (sign in first)", false));
    }
    kids.push(Node::sep(ID_SEP3));
    kids.push(Node::item(ID_QUIT, "Quit ZO-AI", true));
    Node { id: ID_ROOT, label: String::new(), sep: false, enabled: true, kids }
}

/// The signature without the ids of the lines that come and go, so an unchanged menu keeps its ids.
fn signature(n: &Node) -> String {
    let mut s = String::new();
    fn go(n: &Node, out: &mut String) {
        let fixed = n.id < FIRST_DYNAMIC;
        out.push_str(&format!("[{}|{}|{}|{}", if fixed { n.id } else { -1 }, n.label, n.sep, n.enabled));
        for k in &n.kids {
            go(k, out);
        }
        out.push(']');
    }
    go(n, &mut s);
    s
}

fn props_of(n: &Node) -> HashMap<String, Variant> {
    let mut p: HashMap<String, Variant> = HashMap::new();
    if n.id == ID_ROOT {
        p.insert("children-display".into(), "submenu".to_variant());
        return p;
    }
    if n.sep {
        p.insert("type".into(), "separator".to_variant());
        return p;
    }
    p.insert("label".into(), n.label.to_variant());
    p.insert("enabled".into(), n.enabled.to_variant());
    p.insert("visible".into(), true.to_variant());
    if !n.kids.is_empty() {
        p.insert("children-display".into(), "submenu".to_variant());
    }
    p
}

fn layout_variant(n: &Node, depth: i32) -> Variant {
    let kids: Vec<Variant> = if depth == 0 { Vec::new() } else { n.kids.iter().map(|k| Variant::from_variant(&layout_variant(k, depth - 1))).collect() };
    let kids = Variant::array_from_iter_with_type(VariantTy::VARIANT, kids);
    Variant::tuple_from_iter([n.id.to_variant(), props_of(n).to_variant(), kids])
}

/// Looks at what is running and, when the menu is different, makes the new one and tells the panel.
fn refresh(force: bool) -> bool {
    let items = running_items();
    let waiting = crate::ask::pending() > 0;
    let (changed, conn, rev, new_status, tooltip_changed) = ST.with(|s| {
        let mut st = s.borrow_mut();
        let mut tmp = State {
            conn: None,
            name_ok: false,
            watcher: false,
            registered: false,
            registering: false,
            host: false,
            rev: 0,
            root: Node::sep(0),
            sig: String::new(),
            stops: HashMap::new(),
            next_id: st.next_id,
            status: "Active",
            tooltip: (String::new(), String::new()),
            pixmap: None,
        };
        let root = build(&mut tmp, &items, waiting);
        let sig = signature(&root);
        let changed = sig != st.sig;
        if changed || force {
            st.next_id = tmp.next_id;
            for (k, v) in tmp.stops {
                st.stops.insert(k, v);
            }
            if st.stops.len() > 400 {
                // stale ids of old menus: keep only the ones of the new menu
                let mut live = HashMap::new();
                collect_ids(&root, &mut |id| {
                    if let Some(a) = st.stops.get(&id) {
                        live.insert(id, *a);
                    }
                });
                st.stops = live;
            }
            if st.next_id > 1_000_000 {
                st.next_id = FIRST_DYNAMIC;
            }
            st.root = root;
            st.sig = sig;
            st.rev = st.rev.wrapping_add(1);
        }
        let status = if waiting { "NeedsAttention" } else { "Active" };
        let status_changed = status != st.status;
        st.status = status;
        let tip = if items.is_empty() { ("ZO-AI".to_string(), "Nothing is running".to_string()) } else { ("ZO-AI".to_string(), format!("{} running", items.len())) };
        let tip_changed = tip != st.tooltip;
        st.tooltip = tip;
        (changed, st.conn.clone().filter(|_| st.registered), st.rev, status_changed.then_some(status), tip_changed)
    });
    if let Some(c) = conn {
        if changed {
            let _ = c.emit_signal(None, MENU_PATH, MENU_IFACE, "LayoutUpdated", Some(&(rev, 0i32).to_variant()));
        }
        if let Some(s) = new_status {
            let _ = c.emit_signal(None, ITEM_PATH, ITEM_IFACE, "NewStatus", Some(&(s,).to_variant()));
        }
        if tooltip_changed {
            let _ = c.emit_signal(None, ITEM_PATH, ITEM_IFACE, "NewToolTip", None);
        }
    }
    changed
}

fn collect_ids(n: &Node, f: &mut dyn FnMut(i32)) {
    f(n.id);
    for k in &n.kids {
        collect_ids(k, f);
    }
}

// ---------------------------------------------------------------- the icon

/// The mark as an ARGB pixmap, for panels that cannot find the "zo-ai" icon in the theme.
fn pixmap() -> Variant {
    let cached = ST.with(|s| s.borrow().pixmap.clone());
    if let Some(p) = cached {
        return p;
    }
    let bytes = glib::Bytes::from_static(include_bytes!("../data/zo-ai-mark.png"));
    let made = gtk::gdk::Texture::from_bytes(&bytes).ok().map(|t| {
        let (w, h) = (t.width(), t.height());
        let mut d = gtk::gdk::TextureDownloader::new(&t);
        d.set_format(gtk::gdk::MemoryFormat::A8r8g8b8Premultiplied);
        let (data, stride) = d.download_bytes();
        let mut argb = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            argb.extend_from_slice(&data[y * stride..y * stride + (w as usize) * 4]);
        }
        let one = Variant::tuple_from_iter([w.to_variant(), h.to_variant(), argb.to_variant()]);
        Variant::array_from_iter_with_type(VariantTy::new("(iiay)").unwrap(), [one])
    });
    let v = made.unwrap_or_else(|| Variant::array_from_iter_with_type(VariantTy::new("(iiay)").unwrap(), Vec::<Variant>::new()));
    ST.with(|s| s.borrow_mut().pixmap = Some(v.clone()));
    v
}

fn empty_pixmap() -> Variant {
    Variant::array_from_iter_with_type(VariantTy::new("(iiay)").unwrap(), Vec::<Variant>::new())
}

fn item_property(name: &str) -> Variant {
    match name {
        "Category" => "ApplicationStatus".to_variant(),
        "Id" => "zo-ai".to_variant(),
        "Title" => "ZO-AI".to_variant(),
        "Status" => ST.with(|s| s.borrow().status).to_variant(),
        "WindowId" => 0u32.to_variant(),
        "IconName" | "AttentionIconName" => "zo-ai".to_variant(),
        "IconPixmap" | "AttentionIconPixmap" => pixmap(),
        "OverlayIconName" | "AttentionMovieName" => "".to_variant(),
        "OverlayIconPixmap" => empty_pixmap(),
        "ToolTip" => {
            let (t, d) = ST.with(|s| s.borrow().tooltip.clone());
            Variant::tuple_from_iter(["zo-ai".to_variant(), empty_pixmap(), t.to_variant(), d.to_variant()])
        }
        "ItemIsMenu" => false.to_variant(),
        "Menu" => glib::variant::ObjectPath::try_from(MENU_PATH).unwrap().to_variant(),
        _ => 0u32.to_variant(),
    }
}

// ---------------------------------------------------------------- what the menu does

fn on_event(id: i32, event: &str) {
    if event != "clicked" {
        return;
    }
    match id {
        ID_OPEN | ID_APPROVAL => {
            glib::idle_add_local_once(crate::raise_main);
        }
        ID_ACTIVITY => {
            glib::idle_add_local_once(crate::open_activity);
        }
        ID_QUIT => {
            glib::idle_add_local_once(crate::request_quit);
        }
        ID_WEB => {
            let on = zo_core::link::config().enabled;
            if let Err(e) = zo_core::link::set_enabled(!on) {
                eprintln!("zo-ai: tray: web control: {e}");
            }
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), || {
                refresh(false);
            });
        }
        _ => {
            let target = ST.with(|s| s.borrow().stops.get(&id).copied());
            if let Some(a) = target {
                let ok = activity::stop(a);
                eprintln!("zo-ai: tray: stop {a}: {}", if ok { "asked" } else { "nothing to stop" });
                glib::timeout_add_local_once(std::time::Duration::from_millis(400), || {
                    refresh(false);
                });
            }
        }
    }
}

fn menu_call(method: &str, params: &Variant, inv: gio::DBusMethodInvocation) {
    match method {
        "GetLayout" => {
            let (parent, depth, _names) = params.get::<(i32, i32, Vec<String>)>().unwrap_or((0, -1, Vec::new()));
            refresh(false);
            let reply = ST.with(|s| {
                let st = s.borrow();
                st.root.find(parent).map(|n| Variant::tuple_from_iter([st.rev.to_variant(), layout_variant(n, depth)]))
            });
            match reply {
                Some(r) => inv.return_value(Some(&r)),
                None => inv.return_dbus_error("com.canonical.dbusmenu.Error", "no such menu item"),
            }
        }
        "GetGroupProperties" => {
            let (ids, _names) = params.get::<(Vec<i32>, Vec<String>)>().unwrap_or_default();
            let list = ST.with(|s| {
                let st = s.borrow();
                let ids: Vec<i32> = if ids.is_empty() {
                    let mut v = Vec::new();
                    collect_ids(&st.root, &mut |i| v.push(i));
                    v
                } else {
                    ids
                };
                ids.iter()
                    .filter_map(|i| st.root.find(*i))
                    .map(|n| Variant::tuple_from_iter([n.id.to_variant(), props_of(n).to_variant()]))
                    .collect::<Vec<_>>()
            });
            let arr = Variant::array_from_iter_with_type(VariantTy::new("(ia{sv})").unwrap(), list);
            inv.return_value(Some(&Variant::tuple_from_iter([arr])));
        }
        "GetProperty" => {
            let (id, name) = params.get::<(i32, String)>().unwrap_or_default();
            let v = ST.with(|s| s.borrow().root.find(id).and_then(|n| props_of(n).remove(&name)));
            match v {
                Some(v) => inv.return_value(Some(&Variant::tuple_from_iter([Variant::from_variant(&v)]))),
                None => inv.return_dbus_error("com.canonical.dbusmenu.Error", "no such property"),
            }
        }
        "Event" => {
            if let Some((id, ev, _data, _ts)) = params.get::<(i32, String, Variant, u32)>() {
                on_event(id, &ev);
            }
            inv.return_value(None);
        }
        "EventGroup" => {
            if let Some(events) = params.child_value(0).iter().map(|e| e.get::<(i32, String, Variant, u32)>()).collect::<Option<Vec<_>>>() {
                for (id, ev, _, _) in events {
                    on_event(id, &ev);
                }
            }
            inv.return_value(Some(&Variant::tuple_from_iter([Vec::<i32>::new().to_variant()])));
        }
        "AboutToShow" => {
            let changed = refresh(false);
            inv.return_value(Some(&(changed,).to_variant()));
        }
        "AboutToShowGroup" => {
            let changed = refresh(false);
            let ids = params.child_value(0).get::<Vec<i32>>().unwrap_or_default();
            let updated = if changed { ids } else { Vec::new() };
            inv.return_value(Some(&(updated, Vec::<i32>::new()).to_variant()));
        }
        _ => inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
    }
}

fn item_call(method: &str, inv: gio::DBusMethodInvocation) {
    match method {
        "Activate" => {
            glib::idle_add_local_once(crate::toggle_window);
        }
        "SecondaryActivate" => {
            glib::idle_add_local_once(crate::raise_main);
        }
        "ContextMenu" | "Scroll" => {}
        _ => {
            inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method");
            return;
        }
    }
    inv.return_value(None);
}

// ---------------------------------------------------------------- starting

fn try_register() {
    let go = ST.with(|s| {
        let st = s.borrow();
        st.name_ok && st.watcher && !st.registered && !st.registering
    });
    if !go {
        return;
    }
    let Some(conn) = ST.with(|s| s.borrow().conn.clone()) else { return };
    ST.with(|s| s.borrow_mut().registering = true);
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    conn.call(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        WATCHER,
        "RegisterStatusNotifierItem",
        Some(&(name,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        3000,
        gio::Cancellable::NONE,
        |r| match r {
            Ok(_) => {
                ST.with(|s| {
                    let mut st = s.borrow_mut();
                    st.registering = false;
                    st.registered = true;
                });
                check_host();
                refresh(true);
                eprintln!("zo-ai: tray icon registered");
            }
            // tried again by the 30 second look (quietly: one line, no storm)
            Err(e) => {
                ST.with(|s| s.borrow_mut().registering = false);
                eprintln!("zo-ai: tray: the panel did not take the icon: {e}");
            }
        },
    );
}

/// Asks the watcher whether a tray that draws icons is there (the property IsStatusNotifierHostRegistered).
fn check_host() {
    let Some(conn) = ST.with(|s| s.borrow().conn.clone()) else { return };
    conn.call(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&(WATCHER, "IsStatusNotifierHostRegistered").to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        3000,
        gio::Cancellable::NONE,
        |r| {
            let host = r
                .ok()
                .and_then(|v| v.child_value(0).as_variant())
                .and_then(|v| v.get::<bool>())
                .unwrap_or(false);
            let changed = ST.with(|s| std::mem::replace(&mut s.borrow_mut().host, host) != host);
            if changed {
                refresh(true);
            }
        },
    );
}

/// Puts the icon in the panel when there is one that can show it, now or later. Call once, on the main thread.
pub fn start() {
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, |conn| {
        let Ok(conn) = conn else { return };
        let (Ok(sni), Ok(menu)) = (gio::DBusNodeInfo::for_xml(SNI_XML), gio::DBusNodeInfo::for_xml(MENU_XML)) else { return };
        let (Some(sni_if), Some(menu_if)) = (sni.lookup_interface(ITEM_IFACE), menu.lookup_interface(MENU_IFACE)) else { return };
        let ok1 = conn
            .register_object(ITEM_PATH, &sni_if)
            .method_call(|_, _, _, _, method, _, inv| item_call(method, inv))
            .property(|_, _, _, _, name| item_property(name))
            .build();
        let ok2 = conn
            .register_object(MENU_PATH, &menu_if)
            .method_call(|_, _, _, _, method, params, inv| menu_call(method, &params, inv))
            .property(|_, _, _, _, name| match name {
                "Version" => 3u32.to_variant(),
                "TextDirection" => "ltr".to_variant(),
                "Status" => "normal".to_variant(),
                _ => Vec::<String>::new().to_variant(),
            })
            .build();
        if ok1.is_err() || ok2.is_err() {
            return;
        }
        ST.with(|s| s.borrow_mut().conn = Some(conn.clone()));
        let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        std::mem::forget(gio::bus_own_name_on_connection(
            &conn,
            &name,
            gio::BusNameOwnerFlags::NONE,
            |_, _| {
                ST.with(|s| s.borrow_mut().name_ok = true);
                try_register();
            },
            |_, _| {},
        ));
        // the panel (or its tray part) may come later, or restart: this is called each time it appears
        std::mem::forget(gio::bus_watch_name_on_connection(
            &conn,
            WATCHER,
            gio::BusNameWatcherFlags::NONE,
            |_, _, _| {
                ST.with(|s| s.borrow_mut().watcher = true);
                try_register();
            },
            |_, _| {
                ST.with(|s| {
                    let mut st = s.borrow_mut();
                    st.watcher = false;
                    st.registered = false;
                    st.registering = false;
                    st.host = false;
                });
            },
        ));
        // a tray that draws the icons may start after the watcher (at login) or go: ask when it says so, and every 30 seconds
        let _ = conn.signal_subscribe(
            Some(WATCHER),
            Some(WATCHER),
            None,
            Some("/StatusNotifierWatcher"),
            None,
            gio::DBusSignalFlags::NONE,
            |_, _, _, _, _, _| check_host(),
        );
        glib::timeout_add_seconds_local(30, || {
            if ST.with(|s| s.borrow().watcher) {
                try_register();
                check_host();
            }
            glib::ControlFlow::Continue
        });
        // look at what is running once a second; a change goes to the panel (LayoutUpdated) at most that often
        glib::timeout_add_seconds_local(1, || {
            if present() {
                refresh(false);
            }
            glib::ControlFlow::Continue
        });
    });
}
