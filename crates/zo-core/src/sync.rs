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
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// Remembers that the item was deleted now.
pub fn tombstone(kind: &str, id: &str) {
    tomb_set(&key(kind, id), chats::now_millis());
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
    let mut out = json!({"chats": {}, "memory": {}, "personalities": {}, "deleted": s.deleted});
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
            let c: chats::Chat = serde_json::from_value(doc.clone()).map_err(bad)?;
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
    let k = key(kind, id);
    let tomb = tombstones().get(&k).copied();
    if tomb.is_some_and(|t| t >= up) || local_updated(kind, id).is_some_and(|l| l >= up) {
        return Ok(false);
    }
    match parsed {
        Parsed::Chat(c) => {
            if !chats::fits(up) {
                return Ok(false);
            }
            chats::import(c);
        }
        Parsed::State(enabled, every) => memory::change(|s| {
            s.enabled = enabled;
            s.every = every;
            s.state_updated = up;
        }),
        Parsed::Note(i) => memory::change(|s| {
            s.items.retain(|x| x.id != i.id);
            s.items.push(i);
        }),
        Parsed::Person(p) => {
            if personality::builtin().iter().any(|b| b.id == p.id) {
                return Err("That is a built-in personality.".into());
            }
            personality::import(p);
        }
    }
    if tomb.is_some() {
        tomb_clear(&k);
    }
    Ok(true)
}

/// Applies a deletion that came from the other side: remembers it, and removes the item here unless it changed after `at`.
/// Returns whether an item was removed.
pub fn apply_delete(kind: &str, id: &str, at: u64) -> Result<bool, String> {
    checked(kind, id)?;
    if kind == "memory" && id == STATE_ID {
        return Err("The memory state cannot be deleted.".into());
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
    pub url: String,
    pub enabled: bool,
}

fn config_path() -> std::path::PathBuf {
    crate::audit::config_dir().join("sync.json")
}

pub fn config() -> Config {
    let v: Value = std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    Config { url: v.get("url").and_then(Value::as_str).unwrap_or("").to_string(), enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(false) }
}

/// The server address in its clean form (no trailing slash). The access token goes with every call, so only https is accepted
/// (plain http only for this computer itself).
pub fn clean_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/').to_string();
    let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"].iter().any(|p| u.strip_prefix(p).is_some_and(|rest| rest.is_empty() || rest.starts_with(':')));
    if u.starts_with("https://") && u.len() > 10 || local {
        Ok(u)
    } else {
        Err("The server address must start with https:// (so the access token is not sent in the open).".into())
    }
}

pub fn save_config(url: &str, enabled: bool) -> Result<(), String> {
    let url = if url.trim().is_empty() && !enabled { String::new() } else { clean_url(url)? };
    let p = config_path();
    std::fs::create_dir_all(p.parent().unwrap_or(&p)).map_err(|e| e.to_string())?;
    std::fs::write(&p, json!({"url": url, "enabled": enabled}).to_string()).map_err(|e| e.to_string())
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
    json!({"enabled": c.enabled, "url": c.url, "last_ok": s.last_ok, "error": s.error, "pulled": s.pulled, "pushed": s.pushed, "deleted": s.deleted, "running": s.running})
}

fn post(base: &str, token: &str, path: &str, body: &Value) -> Result<Value, String> {
    let url = format!("{base}{path}");
    match ureq::post(&url).timeout(Duration::from_secs(20)).set("Authorization", &format!("Bearer {token}")).send_json(body.clone()) {
        Ok(r) => r.into_json::<Value>().map_err(|_| "The server's answer was not understood.".to_string()),
        Err(ureq::Error::Status(401, _)) => Err("The server refused the access token.".into()),
        Err(ureq::Error::Status(c, r)) => {
            let msg = r.into_json::<Value>().ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
            Err(msg.unwrap_or_else(|| format!("The server answered {c}.")))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("Cannot reach the server: {}", t.kind())),
    }
}

/// One round: compare with the server and carry out the plan. Returns (pulled, pushed, deleted on either side).
pub fn sync_with(base: &str, token: &str) -> Result<(usize, usize, usize), String> {
    let base = clean_url(base)?;
    let remote = side_from_json(&post(&base, token, "/api/sync/manifest", &json!({}))?)?;
    let local = local_side();
    let plan = plan(&local, &remote);
    let (mut pulled, mut pushed, mut deleted) = (0, 0, 0);
    let split = |k: &str| split_key(k).map(|(a, b)| (a.to_string(), b.to_string())).ok_or_else(|| "The server named an unknown kind of item.".to_string());
    for (k, at) in &plan.delete_local {
        let (kind, id) = split(k)?;
        if apply_delete(&kind, &id, *at)? {
            deleted += 1;
        }
    }
    for (k, at) in &plan.delete_remote {
        let (kind, id) = split(k)?;
        let r = post(&base, token, "/api/sync/delete", &json!({"kind": kind, "id": id, "at": at}))?;
        if r.get("deleted").and_then(Value::as_bool) == Some(true) {
            deleted += 1;
        }
    }
    for k in &plan.pull {
        let (kind, id) = split(k)?;
        let up = remote.items.get(k).copied().unwrap_or(0);
        if REFUSED.lock().unwrap_or_else(|e| e.into_inner()).contains(&(format!("pull {k}"), up)) {
            continue;
        }
        let r = post(&base, token, "/api/sync/get", &json!({"kind": kind, "id": id}))?;
        let doc = r.get("doc").filter(|d| d.is_object()).ok_or("The server has no such document any more.")?;
        if put_doc(&kind, &id, doc)? {
            pulled += 1;
        } else {
            REFUSED.lock().unwrap_or_else(|e| e.into_inner()).insert((format!("pull {k}"), up));
        }
    }
    for k in &plan.push {
        let (kind, id) = split(k)?;
        let up = local.items.get(k).copied().unwrap_or(0);
        if REFUSED.lock().unwrap_or_else(|e| e.into_inner()).contains(&(format!("push {k}"), up)) {
            continue;
        }
        let Some(doc) = get_doc(&kind, &id) else { continue };
        let r = post(&base, token, "/api/sync/put", &json!({"kind": kind, "id": id, "doc": doc}))?;
        if r.get("applied").and_then(Value::as_bool) == Some(true) {
            pushed += 1;
        } else {
            REFUSED.lock().unwrap_or_else(|e| e.into_inner()).insert((format!("push {k}"), up));
        }
    }
    Ok((pulled, pushed, deleted))
}

/// Syncs now, with the saved address and token, and records the result in `status()`. Errors change nothing else.
pub fn sync_now() -> Result<(usize, usize, usize), String> {
    let Ok(_run) = RUN.try_lock() else { return Err("A sync is already running.".into()) };
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).running = true;
    let cfg = config();
    let result = match (cfg.url.is_empty(), token()) {
        (true, _) => Err("No server address is set.".to_string()),
        (_, None) => Err("No access token is saved.".to_string()),
        (false, Some(t)) => sync_with(&cfg.url, &t),
    };
    let mut s = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    s.running = false;
    match &result {
        Ok((a, b, c)) => *s = Status { last_ok: chats::now_millis(), error: String::new(), pulled: *a, pushed: *b, deleted: *c, running: false },
        Err(e) => s.error = e.clone(),
    }
    result
}

/// Starts the background loop (once): every 30 seconds, when the sync is switched on.
pub fn start() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(5));
            loop {
                if config().enabled {
                    let _ = sync_now();
                }
                std::thread::sleep(Duration::from_secs(30));
            }
        });
    });
}

#[cfg(test)]
mod tests {
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
}
