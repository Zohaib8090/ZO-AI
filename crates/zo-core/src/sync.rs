//! Keeps the chats, memory notes and personalities of this computer and of the ZO-AI server the same (see docs/SYNC.md).
//!
//! * The **planner** (`plan`) is pure: from what each side holds (item id -> last change, and tombstones: item id -> when it was
//!   deleted) it says what to pull, push and delete. The newest change wins; a tombstone that is not older than the item deletes it.
//! * The **documents** (`local_side`, `get_doc`, `put_doc`, `apply_delete`) read and write the real stores. The server answers
//!   its /api/sync/* calls with the same functions, so both sides apply the same rules.
//! * The **client** (`start`, `sync_now`) runs on the computer: every 30 seconds it asks the server for its manifest, makes a plan
//!   and carries it out. On any error it stops and changes nothing else; nothing is ever deleted because a server looked empty
//!   (only a tombstone deletes).
//!
//! Item keys are `kind:id` with kind `chats`, `memory` or `personalities`. Memory also has one pseudo item, `memory:_state`
//! (is the memory on, and how often the agent reads a chat). API keys and the keys file are never synced.

use crate::store::{chats, memory, personality};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Duration;

pub const KINDS: [&str; 3] = ["chats", "memory", "personalities"];
/// The memory's small state (on/off, how often) travels as a note with this id.
pub const STATE_ID: &str = "_state";
/// Tombstones are forgotten after this long (a computer that was off for longer may bring a deleted item back).
const TOMB_KEEP_MS: u64 = 90 * 86_400_000;
/// A change or deletion stamped further ahead than this is refused (a clock that is wrong, or a bad caller): it would block every
/// later edit of the item. It is tried again each round, so it goes through once the time has come.
const MAX_AHEAD_MS: u64 = 3_600_000;

fn too_far_ahead(at: u64) -> bool {
    at > chats::now_millis().saturating_add(MAX_AHEAD_MS)
}

/// `"kind:id"` -> a time in milliseconds.
pub type Times = BTreeMap<String, u64>;

pub fn key(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}

pub fn split_key(k: &str) -> Option<(&str, &str)> {
    k.split_once(':').filter(|(kind, id)| KINDS.contains(kind) && !id.is_empty())
}

// ---------------------------------------------------------------------------------------------------------------------------
// the planner

/// What one side holds: when each item last changed, and which items it knows were deleted (and when).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    pub items: Times,
    pub deleted: Times,
}

/// What to do. `delete_*` hold `(key, time of the deletion)`: apply the tombstone there (it deletes the item if it is not newer).
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub pull: Vec<String>,
    pub push: Vec<String>,
    pub delete_local: Vec<(String, u64)>,
    pub delete_remote: Vec<(String, u64)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.pull.is_empty() && self.push.is_empty() && self.delete_local.is_empty() && self.delete_remote.is_empty()
    }
}

/// Newest change wins. A tombstone at or after an item's last change deletes it (a later edit brings it back). Equal changes: nothing.
/// An item only one side has is copied to the other unless a tombstone says it was deleted.
pub fn plan(local: &Side, remote: &Side) -> Plan {
    let mut keys: BTreeSet<&String> = BTreeSet::new();
    for m in [&local.items, &local.deleted, &remote.items, &remote.deleted] {
        keys.extend(m.keys());
    }
    let mut p = Plan::default();
    for k in keys {
        let (l, r) = (local.items.get(k).copied(), remote.items.get(k).copied());
        let (tl, tr) = (local.deleted.get(k).copied(), remote.deleted.get(k).copied());
        let t = tl.max(tr);
        let dead = |u: Option<u64>| matches!((u, t), (Some(u), Some(t)) if t >= u);
        // a tombstone older than a surviving copy is stale: the newer copy overwrites the other side anyway
        if let Some(t) = t.filter(|t| l.max(r).map_or(true, |n| *t >= n)) {
            if tl.map_or(true, |x| x < t) || dead(l) {
                p.delete_local.push((k.clone(), t));
            }
            if tr.map_or(true, |x| x < t) || dead(r) {
                p.delete_remote.push((k.clone(), t));
            }
        }
        let (l, r) = (l.filter(|_| !dead(l)), r.filter(|_| !dead(r)));
        match (l, r) {
            (Some(a), Some(b)) if a > b => p.push.push(k.clone()),
            (Some(a), Some(b)) if b > a => p.pull.push(k.clone()),
            (Some(_), None) => p.push.push(k.clone()),
            (None, Some(_)) => p.pull.push(k.clone()),
            _ => {}
        }
    }
    p
}

// ---------------------------------------------------------------------------------------------------------------------------
// tombstones

static TOMB_LOCK: Mutex<()> = Mutex::new(());

fn tomb_path() -> std::path::PathBuf {
    crate::audit::data_dir().join("tombstones.json")
}

/// What was deleted here (or told to this side), by `kind:id`.
pub fn tombstones() -> Times {
    std::fs::read_to_string(tomb_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn tomb_edit(f: impl FnOnce(&mut Times)) {
    let _g = TOMB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut t = tombstones();
    f(&mut t);
    let floor = chats::now_millis().saturating_sub(TOMB_KEEP_MS);
    t.retain(|_, at| *at >= floor);
    let p = tomb_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
    let tmp = p.with_extension("json.tmp");
    if let Ok(s) = serde_json::to_string(&t) {
        if !crate::tenant::fits(&p, s.len()) {
            return;
        }
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// Remembers that the item was deleted now. `item_updated` is when it last changed: the deletion is never stamped before that
/// (a clock that went back would otherwise make the deletion look stale and undo it).
pub fn tombstone(kind: &str, id: &str, item_updated: u64) {
    tomb_set(&key(kind, id), chats::now_millis().max(item_updated.saturating_add(1)));
}

/// Remembers a deletion at `at` (the newest time is kept).
pub fn tomb_set(k: &str, at: u64) {
    tomb_edit(|t| {
        let e = t.entry(k.to_string()).or_insert(0);
        *e = (*e).max(at);
    });
}

fn tomb_clear(k: &str) {
    tomb_edit(|t| {
        t.remove(k);
    });
}

// ---------------------------------------------------------------------------------------------------------------------------
// the documents of this side

/// When each item here last changed, and the tombstones.
pub fn local_side() -> Side {
    let mut items = Times::new();
    for c in chats::load() {
        items.insert(key("chats", &c.id), c.updated);
    }
    let m = memory::load();
    for i in &m.items {
        items.insert(key("memory", &i.id), i.updated);
    }
    if m.state_updated > 0 {
        items.insert(key("memory", STATE_ID), m.state_updated);
    }
    for p in personality::load().items {
        items.insert(key("personalities", &p.id), personality::made_at(&p));
    }
    Side { items, deleted: tombstones() }
}

/// The manifest the server sends: `{chats:{id:updated}, memory:{..}, personalities:{..}, deleted:{"chats:id":at}}`.
pub fn manifest_json() -> Value {
    side_to_json(&local_side())
}

pub fn side_to_json(s: &Side) -> Value {
    // `now` is this computer's clock: the other side uses it to see how far its own clock is off
    let mut out = json!({"chats": {}, "memory": {}, "personalities": {}, "deleted": s.deleted, "now": chats::now_millis()});
    for (k, u) in &s.items {
        if let Some((kind, id)) = split_key(k) {
            out[kind][id] = json!(u);
        }
    }
    out
}

/// Reads a manifest. Anything that does not look like one is an error (a broken answer must never look like an empty server).
pub fn side_from_json(v: &Value) -> Result<Side, String> {
    let bad = || "The server's answer is not a sync manifest.".to_string();
    let mut s = Side::default();
    for kind in KINDS {
        for (id, u) in v.get(kind).and_then(Value::as_object).ok_or_else(bad)? {
            s.items.insert(key(kind, id), u.as_u64().ok_or_else(bad)?);
        }
    }
    for (k, at) in v.get("deleted").and_then(Value::as_object).ok_or_else(bad)? {
        s.deleted.insert(k.clone(), at.as_u64().ok_or_else(bad)?);
    }
    Ok(s)
}

fn checked<'a>(kind: &'a str, id: &str) -> Result<&'a str, String> {
    if !KINDS.contains(&kind) || id.is_empty() || id.len() > 200 {
        return Err("Unknown kind of item.".into());
    }
    // on the server the id comes from a client and becomes a document name in the database: a plain, short, unambiguous name
    // ("." and ".." are refused by the database, "/" would be lossy, and "__" is how a long document is cut into parts)
    if crate::tenant::multi_tenant() && (id.len() > 100 || id.contains("__") || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
        return Err("That id is not allowed (letters, digits, - and _ only, at most 100 characters, no double underscore).".into());
    }
    Ok(kind)
}

/// The document of one item, or None when it is not here.
pub fn get_doc(kind: &str, id: &str) -> Option<Value> {
    match kind {
        "chats" => chats::load().into_iter().find(|c| c.id == id).and_then(|c| serde_json::to_value(c).ok()),
        "memory" if id == STATE_ID => {
            let m = memory::load();
            (m.state_updated > 0).then(|| json!({"enabled": m.enabled, "every": m.every, "updated": m.state_updated}))
        }
        "memory" => memory::load().items.into_iter().find(|i| i.id == id).and_then(|i| serde_json::to_value(i).ok()),
        "personalities" => personality::load().items.into_iter().find(|p| p.id == id).map(|p| json!({"id": p.id, "name": p.name, "prompt": p.prompt, "generated": p.generated})),
        _ => None,
    }
}

fn local_updated(kind: &str, id: &str) -> Option<u64> {
    local_side().items.get(&key(kind, id)).copied()
}

/// Applies a document that came from the other side, only if it is newer than what is here (and than a tombstone).
/// Returns whether it was applied. A document that does not fit its kind is an error.
pub fn put_doc(kind: &str, id: &str, doc: &Value) -> Result<bool, String> {
    checked(kind, id)?;
    let bad = |e: serde_json::Error| format!("The {kind} document is not valid: {e}");
    enum Parsed {
        Chat(chats::Chat),
        State(bool, usize),
        Note(memory::Item),
        Person(personality::Personality),
    }
    let (up, parsed) = match kind {
        "chats" => {
            let mut c: chats::Chat = serde_json::from_value(doc.clone()).map_err(bad)?;
            if crate::tenant::is_restricted() {
                // pictures are files on this computer's disk: a chat that came from a client names none of them
                crate::ai::agent::strip_images(&mut c.history);
            }
            (c.updated, Parsed::Chat(c))
        }
        "memory" if id == STATE_ID => {
            let up = doc.get("updated").and_then(Value::as_u64).filter(|u| *u > 0).ok_or("The memory state has no time.")?;
            let enabled = doc.get("enabled").and_then(Value::as_bool).ok_or("The memory state is not valid.")?;
            let every = doc.get("every").and_then(Value::as_u64).unwrap_or(3).clamp(1, 1000) as usize;
            (up, Parsed::State(enabled, every))
        }
        "memory" => {
            let i: memory::Item = serde_json::from_value(doc.clone()).map_err(bad)?;
            (i.updated, Parsed::Note(i))
        }
        _ => {
            let p: personality::Personality = serde_json::from_value(doc.clone()).map_err(bad)?;
            (personality::made_at(&p), Parsed::Person(p))
        }
    };
    let id_in_doc = match &parsed {
        Parsed::Chat(c) => c.id.as_str(),
        Parsed::Note(i) => i.id.as_str(),
        Parsed::Person(p) => p.id.as_str(),
        Parsed::State(..) => id,
    };
    if id_in_doc != id {
        return Err("The document's id is not the one asked for.".into());
    }
    if too_far_ahead(up) {
        return Err("That item is dated in the future (is a clock wrong?), so it is not taken in yet.".into());
    }
    let k = key(kind, id);
    let tomb = tombstones().get(&k).copied();
    if tomb.is_some_and(|t| t >= up) || local_updated(kind, id).is_some_and(|l| l >= up) {
        return Ok(false);
    }
    // the last check and the write happen together (under the store's own lock), so a change made here meanwhile is not overwritten
    let applied = match parsed {
        Parsed::Chat(c) => chats::import_if_newer(c),
        Parsed::State(enabled, every) => memory::change(|s| {
            if s.state_updated >= up {
                return false;
            }
            s.enabled = enabled;
            s.every = every;
            s.state_updated = up;
            true
        }),
        Parsed::Note(i) => memory::change(|s| {
            if s.items.iter().any(|x| x.id == i.id && x.updated >= i.updated) {
                return false;
            }
            s.items.retain(|x| x.id != i.id);
            s.items.push(i);
            true
        }),
        Parsed::Person(p) => {
            if personality::builtin().iter().any(|b| b.id == p.id) {
                return Err("That is a built-in personality.".into());
            }
            personality::import(p);
            true
        }
    };
    if applied && tomb.is_some() {
        tomb_clear(&k);
    }
    Ok(applied)
}

/// Applies a deletion that came from the other side: remembers it, and removes the item here unless it changed after `at`.
/// Returns whether an item was removed.
pub fn apply_delete(kind: &str, id: &str, at: u64) -> Result<bool, String> {
    checked(kind, id)?;
    if kind == "memory" && id == STATE_ID {
        return Err("The memory state cannot be deleted.".into());
    }
    if too_far_ahead(at) {
        return Err("That deletion is dated in the future (is a clock wrong?), so it is not taken in yet.".into());
    }
    tomb_set(&key(kind, id), at);
    let Some(l) = local_updated(kind, id) else { return Ok(false) };
    if at < l {
        return Ok(false);
    }
    match kind {
        "chats" => chats::delete_quiet(id),
        "memory" => memory::change(|s| s.items.retain(|i| i.id != id)),
        _ => personality::remove_quiet(id),
    }
    Ok(true)
}

// ---------------------------------------------------------------------------------------------------------------------------
// the client (this computer talking to the server)

/// ~/.config/zo-ai/sync.json: where the server is and whether to sync.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// A server address chosen by hand (empty = the default one, see `login::server_url`).
    pub url: String,
    pub enabled: bool,
    /// The signed-in account (see `login`); empty when nobody is signed in. Never a secret.
    pub email: String,
}

fn config_path() -> std::path::PathBuf {
    crate::audit::config_dir().join("sync.json")
}

pub fn config() -> Config {
    let v: Value = std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Config { url: text("url"), enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(false), email: text("email") }
}

/// The server address in its clean form (no trailing slash). The device token goes with every call, so only https is accepted
/// (plain http only for this computer itself).
pub fn clean_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/').to_string();
    let err = || "The server address must start with https:// (so the device token is not sent in the open).".to_string();
    let (https, rest) = match (u.strip_prefix("https://"), u.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (None, Some(r)) => (false, r),
        _ => return Err(err()),
    };
    // the host is what stands before the first `/ ? # \`; `user@host` tricks (http://localhost:80@evil.example) are refused, and
    // plain http is only for this computer itself, matched on the whole host
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', ' ']) || u.contains('\\') || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(err());
    }
    if https {
        return Ok(u);
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix("[::1]") {
        ("[::1]", if rest.is_empty() { None } else { Some(rest.strip_prefix(':').unwrap_or("x")) })
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    let port_ok = port.map_or(true, |p| !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()));
    if port_ok && matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        Ok(u)
    } else {
        Err(err())
    }
}

pub fn save_config(url: &str, enabled: bool) -> Result<(), String> {
    let url = if url.trim().is_empty() && !enabled { String::new() } else { clean_url(url)? };
    write_config(&url, enabled, &config().email)
}

/// Writes sync.json as it is (the caller has checked the address). The credential is never in this file.
pub(crate) fn write_config(url: &str, enabled: bool, email: &str) -> Result<(), String> {
    let p = config_path();
    let dir = p.parent().unwrap_or(&p);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, json!({"url": url, "enabled": enabled, "email": email}).to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// Forgets what the last rounds said (after signing out): the status line, the skipped items and the refused pushes.
pub fn reset_status() {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = Status::default();
    SKIPPED.lock().unwrap_or_else(|e| e.into_inner()).clear();
    REFUSED.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Asks the background loop to run a round within a second (set when a person signs in).
static KICK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn kick() {
    KICK.store(true, std::sync::atomic::Ordering::SeqCst);
}

const TOKEN_ID: &str = "sync-token";

pub fn token() -> Option<String> {
    crate::ai::keys::get(TOKEN_ID)
}

pub fn set_token(token: &str) -> Result<(), String> {
    crate::ai::keys::set(TOKEN_ID, token)
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    /// When the last sync worked (ms since 1970), 0 = never.
    pub last_ok: u64,
    /// Why the last try failed (empty when it worked).
    pub error: String,
    pub pulled: usize,
    pub pushed: usize,
    pub deleted: usize,
    pub running: bool,
}

static STATUS: Mutex<Status> = Mutex::new(Status { last_ok: 0, error: String::new(), pulled: 0, pushed: 0, deleted: 0, running: false });
static RUN: Mutex<()> = Mutex::new(());
/// Pushes the server refused as "not newer / does not fit": not tried again for the same version of the item.
static REFUSED: Mutex<BTreeSet<(String, u64)>> = Mutex::new(BTreeSet::new());

pub fn status() -> Status {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The status as the `sync_status` service op answers it. The token itself is never in it.
pub fn status_json() -> Value {
    let (c, s) = (config(), status());
    json!({"enabled": c.enabled, "url": c.url, "email": c.email, "last_ok": s.last_ok, "error": s.error, "pulled": s.pulled, "pushed": s.pushed, "deleted": s.deleted, "running": s.running})
}

/// What the server says for a 401: a device token that is wrong or was revoked.
pub const TOKEN_REFUSED: &str = "This device is no longer signed in (it was removed from your account, or the sign-in is wrong). Sign in again.";

/// Checks the shape of a device token (made on the web app's Account page): it starts with `zd_` and is 20..200 characters long,
/// without spaces. Returns the trimmed token, or a friendly message.
pub fn check_device_token(token: &str) -> Result<String, String> {
    let t = token.trim();
    if !t.starts_with("zd_") {
        return Err("That is not a device token: it must start with zd_. Make one in the web app: Settings > Account > Create device token.".into());
    }
    if t.len() < 20 || t.len() > 200 || t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("That device token looks incomplete or damaged (it should be about 47 characters, no spaces). Copy it again from the web app.".into());
    }
    Ok(t.to_string())
}

/// Seconds to wait for a 429 answer, from its Retry-After header (1..=60, default 5).
pub fn retry_after_secs(header: Option<&str>) -> u64 {
    header.and_then(|h| h.trim().parse::<u64>().ok()).unwrap_or(5).clamp(1, 60)
}

fn post(base: &str, token: &str, path: &str, body: &Value) -> Result<Value, String> {
    let url = format!("{base}{path}");
    let mut waited = false;
    loop {
        match post_once(&url, token, body) {
            Err((429, wait, _)) if !waited => {
                // honour Retry-After once
                waited = true;
                std::thread::sleep(Duration::from_secs(wait));
            }
            Err((429, wait, _)) => return Err(format!("Too many requests, try again in {wait} s.")),
            Err((_, _, e)) => return Err(e),
            Ok(v) => return Ok(v),
        }
    }
}

fn post_once(url: &str, token: &str, body: &Value) -> Result<Value, (u16, u64, String)> {
    let fail = |e: String| (0u16, 0u64, e);
    match ureq::post(url).timeout(Duration::from_secs(20)).set("Authorization", &format!("Bearer {token}")).send_json(body.clone()) {
        Ok(r) => r.into_json::<Value>().map_err(|_| fail("The server's answer was not understood.".to_string())),
        Err(ureq::Error::Status(401, _)) => Err(fail(TOKEN_REFUSED.into())),
        Err(ureq::Error::Status(429, r)) => Err((429, retry_after_secs(r.header("Retry-After")), String::new())),
        // a server that is down or restarting is not an answer about the item
        Err(ureq::Error::Status(c, _)) if c >= 500 => Err(fail(format!("The server answered {c}."))),
        Err(ureq::Error::Status(c, r)) => {
            let msg = r.into_json::<Value>().ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
            Err(fail(msg.unwrap_or_else(|| format!("The server answered {c}."))))
        }
        Err(ureq::Error::Transport(t)) => Err(fail(format!("Cannot reach the server: {}", t.kind()))),
    }
}

/// How far this computer's clock is behind the server's (add it to a local time to get the server's), from the server's `now` in
/// its manifest and the middle of the time the request took. Under 5 seconds counts as none; it is kept to whole seconds so that
/// it does not change from round to round.
pub fn skew_ms(sent: u64, received: u64, server_now: u64) -> i64 {
    let mid = sent / 2 + received / 2;
    let d = server_now as i64 - mid as i64;
    if d.abs() < 5_000 {
        0
    } else {
        d / 1000 * 1000
    }
}

fn shift_time(t: u64, by: i64) -> u64 {
    if by >= 0 {
        t.saturating_add(by as u64)
    } else {
        t.saturating_sub(by.unsigned_abs())
    }
}

/// A side with its times moved onto the other computer's clock. Personalities keep theirs: their time is in their id.
fn shifted(s: &Side, by: i64) -> Side {
    let f = |m: &Times, tomb: bool| -> Times { m.iter().map(|(k, t)| (k.clone(), if !tomb && k.starts_with("personalities:") { *t } else { shift_time(*t, by) })).collect() };
    Side { items: f(&s.items, false), deleted: f(&s.deleted, true) }
}

/// Moves the time inside a document (chats and memory carry one) by `by`.
fn shift_doc(kind: &str, doc: &mut Value, by: i64) {
    if kind != "personalities" {
        if let Some(u) = doc.get("updated").and_then(Value::as_u64) {
            doc["updated"] = json!(shift_time(u, by));
        }
    }
}

/// Things that stop the whole round: the server cannot be reached or does not accept the token. Any other error belongs to one item.
fn is_fatal(e: &str) -> bool {
    e.starts_with("Cannot reach the server") || e == TOKEN_REFUSED || e.starts_with("Too many requests") || e.starts_with("The server's answer") || e.starts_with("The server answered 5")
}

/// The items the last round skipped because of an error of their own (the rest of the round went on).
static SKIPPED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn skipped() -> Vec<String> {
    SKIPPED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// One round: compare with the server and carry out the plan. Returns (pulled, pushed, deleted on either side).
/// An error that belongs to one item (the server refuses it, it does not parse, it is dated in the future) skips that item and
/// goes on with the others; see `skipped()`. Only a failure to reach the server or sign in ends the round.
pub fn sync_with(base: &str, token: &str) -> Result<(usize, usize, usize), String> {
    let base = clean_url(base)?;
    let sent = chats::now_millis();
    let manifest = post(&base, token, "/api/sync/manifest", &json!({}))?;
    let received = chats::now_millis();
    let remote = side_from_json(&manifest)?;
    // the two computers' clocks may differ: times are compared on the server's clock, and documents are moved onto it or back
    let skew = manifest.get("now").and_then(Value::as_u64).map_or(0, |n| skew_ms(sent, received, n));
    let local_raw = local_side();
    let local = shifted(&local_raw, skew);
    let plan = plan(&local, &remote);
    let (mut pulled, mut pushed, mut deleted) = (0, 0, 0);
    let mut skipped: Vec<String> = Vec::new();
    let split = |k: &str| split_key(k).map(|(a, b)| (a.to_string(), b.to_string())).ok_or_else(|| "The server named an unknown kind of item.".to_string());
    let is_refused = |what: &str, k: &str, up: u64| REFUSED.lock().unwrap_or_else(|e| e.into_inner()).contains(&(format!("{what} {k}"), up));
    let refuse = |what: &str, k: &str, up: u64| {
        REFUSED.lock().unwrap_or_else(|e| e.into_inner()).insert((format!("{what} {k}"), up));
    };
    // Err(fatal) ends the round; an item's own error is noted and the next item goes on
    let mut note = |k: &str, e: String| -> Result<(), String> {
        if is_fatal(&e) {
            return Err(e);
        }
        skipped.push(format!("{k}: {e}"));
        Ok(())
    };
    for (k, at) in &plan.delete_local {
        let r = split(k).and_then(|(kind, id)| apply_delete(&kind, &id, shift_time(*at, -skew)));
        match r {
            Ok(true) => deleted += 1,
            Ok(false) => {}
            Err(e) => note(k, e)?,
        }
    }
    for (k, at) in &plan.delete_remote {
        let r = split(k).and_then(|(kind, id)| post(&base, token, "/api/sync/delete", &json!({"kind": kind, "id": id, "at": at})));
        match r {
            Ok(r) if r.get("deleted").and_then(Value::as_bool) == Some(true) => deleted += 1,
            Ok(_) => {}
            Err(e) => note(k, e)?,
        }
    }
    for k in &plan.pull {
        let up = remote.items.get(k).copied().unwrap_or(0);
        if is_refused("pull", k, up) {
            continue;
        }
        let r = split(k).and_then(|(kind, id)| {
            let r = post(&base, token, "/api/sync/get", &json!({"kind": kind, "id": id}))?;
            let mut doc = r.get("doc").filter(|d| d.is_object()).cloned().ok_or("The server has no such document any more.")?;
            shift_doc(&kind, &mut doc, -skew);
            put_doc(&kind, &id, &doc)
        });
        match r {
            Ok(true) => pulled += 1,
            Ok(false) => refuse("pull", k, up),
            Err(e) => note(k, e)?,
        }
    }
    for k in &plan.push {
        let up = local.items.get(k).copied().unwrap_or(0);
        if is_refused("push", k, up) {
            continue;
        }
        let r = split(k).and_then(|(kind, id)| {
            let Some(mut doc) = get_doc(&kind, &id) else { return Ok(None) };
            shift_doc(&kind, &mut doc, skew);
            post(&base, token, "/api/sync/put", &json!({"kind": kind, "id": id, "doc": doc})).map(Some)
        });
        match r {
            Ok(Some(r)) if r.get("applied").and_then(Value::as_bool) == Some(true) => pushed += 1,
            Ok(Some(_)) => refuse("push", k, up),
            Ok(None) => {}
            Err(e) => {
                // the server refused this very version (too big, not valid): not sent again until the item changes
                if !is_fatal(&e) {
                    refuse("push", k, up);
                }
                note(k, e)?
            }
        }
    }
    *SKIPPED.lock().unwrap_or_else(|e| e.into_inner()) = skipped;
    Ok((pulled, pushed, deleted))
}

/// Syncs now, with the saved address and token, and records the result in `status()`. Errors change nothing else.
pub fn sync_now() -> Result<(usize, usize, usize), String> {
    let Ok(_run) = RUN.try_lock() else { return Err("A sync is already running.".into()) };
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).running = true;
    let result = match (crate::login::server_url(), token()) {
        (Err(e), _) => Err(e),
        (_, None) => Err("Not signed in.".to_string()),
        (Ok(url), Some(t)) => sync_with(&url, &t),
    };
    let mut s = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    s.running = false;
    match &result {
        Ok((a, b, c)) => {
            let sk = skipped();
            let error = match sk.first() {
                Some(first) => format!("{} item(s) were skipped. First: {first}", sk.len()),
                None => String::new(),
            };
            *s = Status { last_ok: chats::now_millis(), error, pulled: *a, pushed: *b, deleted: *c, running: false }
        }
        Err(e) => s.error = e.clone(),
    }
    result
}

/// Starts the background loop (once): every 30 seconds, when the sync is switched on.
pub fn start() {
    // the desktop's own sync client: a multi-tenant server has one Sync manager per tenant instead, and no default folders
    if crate::tenant::multi_tenant() {
        return;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| {
            // the first round comes after 5 seconds; a sign-in (`kick`) starts one within a second
            let mut wait = 5;
            loop {
                for _ in 0..wait {
                    if KICK.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                wait = 30;
                if config().enabled && token().is_some() {
                    let _ = sync_now();
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn device_token_shape() {
        let good = format!("zd_{}", "A".repeat(43));
        assert_eq!(check_device_token(&format!("  {good}\n")).unwrap(), good);
        assert!(check_device_token("abc").unwrap_err().contains("zd_"));
        assert!(check_device_token("zd_short").unwrap_err().contains("incomplete"));
        assert!(check_device_token(&format!("zd_{}", "A".repeat(200))).is_err());
        assert!(check_device_token(&format!("zd_{} {}", "A".repeat(20), "B".repeat(20))).is_err());
        assert!(check_device_token("").is_err());
    }

    #[test]
    fn retry_after_is_clamped() {
        assert_eq!(retry_after_secs(Some("7")), 7);
        assert_eq!(retry_after_secs(Some("0")), 1);
        assert_eq!(retry_after_secs(Some("9999")), 60);
        assert_eq!(retry_after_secs(Some("soon")), 5);
        assert_eq!(retry_after_secs(None), 5);
    }

    use super::*;

    fn side(items: &[(&str, u64)], deleted: &[(&str, u64)]) -> Side {
        let m = |a: &[(&str, u64)]| a.iter().map(|(k, v)| (k.to_string(), *v)).collect::<Times>();
        Side { items: m(items), deleted: m(deleted) }
    }
    fn names(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    #[test]
    fn items_only_one_side_has_are_copied() {
        let p = plan(&side(&[("chats:a", 5)], &[]), &side(&[("chats:b", 7)], &[]));
        assert_eq!(names(&p.push), ["chats:a"]);
        assert_eq!(names(&p.pull), ["chats:b"]);
        assert!(p.delete_local.is_empty() && p.delete_remote.is_empty());
    }

    #[test]
    fn an_empty_or_failed_server_never_deletes_anything_here() {
        let p = plan(&side(&[("chats:a", 5), ("memory:m", 6)], &[]), &Side::default());
        assert_eq!(names(&p.push), ["chats:a", "memory:m"]);
        assert!(p.pull.is_empty() && p.delete_local.is_empty());
    }

    #[test]
    fn the_newest_change_wins_and_equal_ones_are_left_alone() {
        let p = plan(&side(&[("chats:a", 9), ("chats:b", 3), ("chats:c", 4)], &[]), &side(&[("chats:a", 5), ("chats:b", 8), ("chats:c", 4)], &[]));
        assert_eq!(names(&p.push), ["chats:a"]);
        assert_eq!(names(&p.pull), ["chats:b"]);
    }

    #[test]
    fn a_tombstone_deletes_the_older_item_on_the_other_side() {
        // deleted here at 10; the server still has the chat from 5
        let p = plan(&side(&[], &[("chats:a", 10)]), &side(&[("chats:a", 5)], &[]));
        assert_eq!(p.delete_remote, [("chats:a".to_string(), 10)]);
        assert!(p.pull.is_empty() && p.push.is_empty() && p.delete_local.is_empty());
        // deleted on the server; here it is still there
        let p = plan(&side(&[("memory:m", 5)], &[]), &side(&[], &[("memory:m", 10)]));
        assert_eq!(p.delete_local, [("memory:m".to_string(), 10)]);
        assert!(p.pull.is_empty() && p.push.is_empty() && p.delete_remote.is_empty());
    }

    #[test]
    fn a_later_edit_beats_the_tombstone_and_a_tie_deletes() {
        // edited at 20 after a deletion at 10 on the other side: it comes back
        let p = plan(&side(&[("chats:a", 20)], &[]), &side(&[], &[("chats:a", 10)]));
        assert_eq!(names(&p.push), ["chats:a"]);
        assert!(p.delete_local.is_empty());
        // deletion exactly at the time of the last change counts as the later
        let p = plan(&side(&[("chats:a", 10)], &[]), &side(&[], &[("chats:a", 10)]));
        assert_eq!(p.delete_local, [("chats:a".to_string(), 10)]);
        assert!(p.push.is_empty());
    }

    #[test]
    fn a_tombstone_is_passed_on_to_a_side_that_has_never_seen_the_item() {
        let p = plan(&side(&[], &[("chats:a", 10)]), &side(&[], &[]));
        assert_eq!(p.delete_remote, [("chats:a".to_string(), 10)]);
        let p = plan(&side(&[], &[]), &side(&[], &[("chats:a", 10)]));
        assert_eq!(p.delete_local, [("chats:a".to_string(), 10)]);
        // both know: nothing to do
        assert!(plan(&side(&[], &[("chats:a", 10)]), &side(&[], &[("chats:a", 10)])).is_empty());
    }

    #[test]
    fn a_deleted_item_is_not_pushed_back_while_the_other_side_still_has_it() {
        // here: deleted at 10. there: the old copy (5). The old copy must not come back, and the next round has nothing to do.
        let p = plan(&side(&[], &[("chats:a", 10)]), &side(&[("chats:a", 5)], &[]));
        assert!(p.pull.is_empty());
        let p2 = plan(&side(&[], &[("chats:a", 10)]), &side(&[], &[("chats:a", 10)]));
        assert!(p2.is_empty());
    }

    #[test]
    fn a_stale_tombstone_next_to_a_newer_item_on_both_sides_settles_in_one_round() {
        let p = plan(&side(&[("chats:a", 20)], &[("chats:a", 10)]), &side(&[("chats:a", 20)], &[]));
        assert!(p.is_empty(), "the old deletion is not worth passing on");
    }

    #[test]
    fn manifests_round_trip_and_reject_garbage() {
        let s = side(&[("chats:a", 1), ("memory:_state", 2), ("personalities:p1", 3)], &[("chats:z", 9)]);
        assert_eq!(side_from_json(&side_to_json(&s)).unwrap(), s);
        assert!(side_from_json(&json!({"ok": false, "error": "x"})).is_err());
        assert!(side_from_json(&json!({"chats": {"a": "x"}, "memory": {}, "personalities": {}, "deleted": {}})).is_err());
        assert_eq!(split_key("chats:a:b"), Some(("chats", "a:b")));
        assert_eq!(split_key("keys:a"), None);
    }

    #[test]
    fn addresses_must_be_https_except_this_computer() {
        assert_eq!(clean_url(" https://zo.onrender.com/ ").unwrap(), "https://zo.onrender.com");
        assert!(clean_url("http://zo.onrender.com").is_err());
        assert!(clean_url("http://127.0.0.1:8799").is_ok());
        assert!(clean_url("http://127.0.0.1.evil.com").is_err());
        for bad in ["http://localhost:80@evil.example/", "http://127.0.0.1:1@evil.example", "http://[::1]:80@evil.example", "https://localhost:80@evil.example", "http://localhost:80\\@evil.example", "http://localhost:abc", "http://localhost:", "http://localhost.evil.com", "http://[::1]x", "http://localhost@evil.example", "https:// x.com"] {
            assert!(clean_url(bad).is_err(), "{bad}");
        }
        for good in ["http://localhost", "http://localhost:8787/", "http://[::1]:8787", "http://[::1]", "https://zo.onrender.com:443"] {
            assert!(clean_url(good).is_ok(), "{good}");
        }
        assert!(clean_url("zo.onrender.com").is_err());
        assert!(clean_url("https://").is_err());
    }

    fn sandbox(name: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        let g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        (g, dir)
    }

    fn chat(id: &str, title: &str, updated: u64) -> Value {
        json!({"id": id, "title": title, "updated": updated, "entries": [{"kind": "msg-user", "text": "hi"}]})
    }

    #[test]
    fn deleting_leaves_a_tombstone_and_a_newer_put_brings_the_item_back() {
        let (_g, dir) = sandbox("tomb");
        assert!(put_doc("chats", "c1", &chat("c1", "one", 100)).unwrap());
        chats::delete("c1");
        let t = tombstones();
        assert!(t["chats:c1"] > 100);
        // an old copy of the chat from the other side is refused
        assert!(!put_doc("chats", "c1", &chat("c1", "one", 100)).unwrap());
        assert!(chats::load().is_empty());
        // a copy edited after the deletion is accepted and the tombstone goes
        let later = t["chats:c1"] + 5;
        assert!(put_doc("chats", "c1", &chat("c1", "one again", later)).unwrap());
        assert_eq!(chats::load()[0].title, "one again");
        assert!(!tombstones().contains_key("chats:c1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn forgetting_a_note_and_clearing_memory_leave_tombstones() {
        let (_g, dir) = sandbox("mem");
        let (id, _) = memory::remember("The person likes green tea in the morning", "preference", "c").unwrap();
        let (id2, _) = memory::remember("The laptop is called Zohara", "fact", "c").unwrap();
        assert!(memory::forget(&id));
        assert!(tombstones().contains_key(&format!("memory:{id}")));
        memory::clear_all();
        assert!(tombstones().contains_key(&format!("memory:{id2}")));
        assert!(local_side().items.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn put_keeps_the_newest_and_refuses_wrong_documents() {
        let (_g, dir) = sandbox("put");
        assert!(put_doc("chats", "c1", &chat("c1", "v2", 200)).unwrap());
        assert!(!put_doc("chats", "c1", &chat("c1", "v1", 100)).unwrap(), "older");
        assert!(!put_doc("chats", "c1", &chat("c1", "same", 200)).unwrap(), "equal");
        assert!(put_doc("chats", "c1", &chat("c1", "v3", 300)).unwrap());
        assert_eq!(chats::load()[0].title, "v3");
        assert!(put_doc("chats", "c1", &chat("other", "x", 400)).is_err(), "id must match");
        assert!(put_doc("keys", "x", &json!({})).is_err());
        assert!(put_doc("chats", "c1", &json!({"id": "c1"})).is_err(), "not a chat");
        // memory notes and the state
        let note = json!({"id": "m1", "text": "likes tea", "kind": "preference", "updated": 50});
        assert!(put_doc("memory", "m1", &note).unwrap());
        assert_eq!(get_doc("memory", "m1").unwrap()["text"], "likes tea");
        assert!(put_doc("memory", STATE_ID, &json!({"enabled": false, "every": 5, "updated": 60})).unwrap());
        let m = memory::load();
        assert!(!m.enabled && m.every == 5);
        assert!(!put_doc("memory", STATE_ID, &json!({"enabled": true, "every": 3, "updated": 10})).unwrap());
        // personalities: only the person's own
        assert!(put_doc("personalities", "p77", &json!({"id": "p77", "name": "Pirate", "prompt": "Sound like a pirate."})).unwrap());
        assert!(put_doc("personalities", "friend", &json!({"id": "friend", "name": "x", "prompt": "y"})).is_err());
        assert_eq!(local_side().items["personalities:p77"], 77);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_remote_delete_removes_older_items_only_and_leaves_no_new_tombstone() {
        let (_g, dir) = sandbox("del");
        let now = chats::now_millis();
        put_doc("chats", "c1", &chat("c1", "x", now - 1000)).unwrap();
        put_doc("chats", "c2", &chat("c2", "y", now - 100)).unwrap();
        assert!(apply_delete("chats", "c1", now - 500).unwrap());
        assert!(!apply_delete("chats", "c2", now - 500).unwrap(), "changed after the deletion");
        assert_eq!(chats::load().len(), 1);
        assert_eq!(tombstones()["chats:c1"], now - 500, "the time of the other side's deletion is kept, not now");
        assert!(apply_delete("memory", STATE_ID, 5).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_local_manifest_lists_what_is_stored() {
        let (_g, dir) = sandbox("man");
        put_doc("chats", "c1", &chat("c1", "x", 100)).unwrap();
        personality::add("Calm", "Sound calm and short in every answer.", false).unwrap();
        let m = manifest_json();
        assert_eq!(m["chats"]["c1"], 100);
        assert_eq!(m["personalities"].as_object().unwrap().len(), 1, "built-in ones are not synced");
        assert!(m["deleted"].as_object().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_tombstone_is_never_stamped_before_the_item_it_deletes() {
        let (_g, dir) = sandbox("skew");
        let now = chats::now_millis();
        // a chat dated half an hour ahead (a clock that was wrong): deleting it must still count as newer than the chat
        assert!(put_doc("chats", "c1", &chat("c1", "x", now + 1_800_000)).unwrap());
        chats::delete("c1");
        assert!(tombstones()["chats:c1"] > now + 1_800_000);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn times_far_in_the_future_are_refused() {
        let (_g, dir) = sandbox("future");
        let now = chats::now_millis();
        assert!(put_doc("chats", "c1", &chat("c1", "x", u64::MAX)).is_err());
        assert!(put_doc("chats", "c1", &chat("c1", "x", now + 2 * MAX_AHEAD_MS)).is_err());
        assert!(apply_delete("chats", "c1", u64::MAX).is_err());
        assert!(put_doc("memory", STATE_ID, &json!({"enabled": true, "every": 3, "updated": u64::MAX})).is_err());
        assert!(tombstones().is_empty() && chats::load().is_empty());
        // a few minutes ahead is a normal clock difference
        assert!(put_doc("chats", "c1", &chat("c1", "x", now + 300_000)).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clock_difference_is_measured_and_times_are_moved() {
        assert_eq!(skew_ms(1_000_000, 1_000_400, 1_000_200), 0);
        assert_eq!(skew_ms(1_000_000, 1_000_400, 1_000_200 + 120_000), 120_000);
        assert_eq!(skew_ms(1_000_000, 1_000_400, 1_000_200 - 120_000), -120_000);
        assert_eq!(shift_time(5, -10), 0);
        let s = shifted(&side(&[("chats:a", 100), ("personalities:p1", 1)], &[("chats:z", 50)]), 20);
        assert_eq!(s.items["chats:a"], 120);
        assert_eq!(s.items["personalities:p1"], 1);
        assert_eq!(s.deleted["chats:z"], 70);
        let mut d = json!({"updated": 100});
        shift_doc("chats", &mut d, -30);
        assert_eq!(d["updated"], 70);
    }

    #[test]
    fn a_chat_that_grew_while_it_was_answered_keeps_the_new_messages() {
        let (_g, dir) = sandbox("merge");
        let e = |k: &str, t: &str| chats::Entry { kind: k.into(), text: t.into() };
        let mut c = chats::Chat { id: "c1".into(), title: "t".into(), updated: 100, entries: vec![e("msg-user", "one")], ..Default::default() };
        chats::upsert(c.clone());
        let base = chats::Base::of(&c);
        // meanwhile the other computer adds a question and its answer (a sync put)
        let mut theirs = c.clone();
        theirs.entries.push(e("msg-user", "from the laptop"));
        theirs.entries.push(e("msg-ai", "laptop answer"));
        theirs.updated = 200;
        assert!(chats::import_if_newer(theirs));
        assert!(!chats::import_if_newer(chats::Chat { id: "c1".into(), updated: 200, ..Default::default() }), "not newer");
        // the answer to the first question arrives
        c.entries.push(e("msg-ai", "server answer"));
        c.updated = 150;
        let id = chats::upsert_checked(c, base);
        assert_eq!(id, "c1");
        let got = &chats::load()[0];
        let texts: Vec<&str> = got.entries.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, ["one", "from the laptop", "laptop answer", "server answer"]);
        assert!(got.updated > 200);
        // a chat that was changed in another way (an edit) is not overwritten: the answer is kept as a copy
        let mut edited = got.clone();
        let base2 = chats::Base::of(&edited);
        let mut other = got.clone();
        other.entries[0].text = "one, edited".into();
        other.updated += 10;
        assert!(chats::import_if_newer(other));
        edited.entries.push(e("msg-ai", "late"));
        edited.updated += 5;
        let id2 = chats::upsert_checked(edited, base2);
        assert_ne!(id2, "c1");
        assert_eq!(chats::load().len(), 2);
        assert_eq!(chats::load().iter().find(|c| c.id == "c1").unwrap().entries[0].text, "one, edited");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A tiny server: answers /api/sync/manifest with a tombstone of a kind nobody knows, and accepts every put.
    fn fake_server() -> String {
        use std::io::{BufRead, BufReader, Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in l.incoming().flatten() {
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                r.read_line(&mut first).unwrap();
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                let answer = if first.contains("/manifest") {
                    json!({"ok": true, "chats": {}, "memory": {}, "personalities": {}, "deleted": {"weird:1": 5}}).to_string()
                } else {
                    json!({"ok": true, "applied": true}).to_string()
                };
                let mut w = stream;
                let _ = write!(w, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len());
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    #[test]
    fn one_item_that_always_fails_does_not_stop_the_others() {
        let (_g, dir) = sandbox("isolate");
        put_doc("chats", "c1", &chat("c1", "x", chats::now_millis() - 5000)).unwrap();
        let url = fake_server();
        let (pulled, pushed, _) = sync_with(&url, "t").unwrap();
        assert_eq!((pulled, pushed), (0, 1), "the chat was still sent");
        assert!(skipped().iter().any(|s| s.starts_with("weird:1")), "{:?}", skipped());
        let _ = std::fs::remove_dir_all(dir);
    }
}
