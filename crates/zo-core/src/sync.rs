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
//! Item keys are `kind:id` with kind `chats`, `memory`, `personalities`, `profiles`, `settings`, `mcp` or `keys`. Memory also has one
//! pseudo item, `memory:_state` (is the memory on, and how often the agent reads a chat); the person's picker preferences are the
//! one item `settings:main`. Profiles travel WITHOUT any key. `mcp` carries remote servers only (name, address, on/off), never a
//! token, a header, a program or an account that is restricted. `keys` (API keys) only travel when the person switched
//! "Also sync my API keys" on on this computer; they are never in any other document.

use crate::ai::{profiles, providers, ui_prefs};
use crate::mcp_client;
use crate::store::{chats, memory, personality};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Duration;

pub const KINDS: [&str; 7] = ["chats", "memory", "personalities", "profiles", "settings", "mcp", "keys"];
/// The kinds that older servers also know (a manifest must list these).
const OLD_KINDS: [&str; 3] = ["chats", "memory", "personalities"];
/// The start of a refusal that is not an error of the item but a rule of the account (a restricted account takes no MCP server and
/// only public https addresses): the computer does not offer it again for the same version and does not list it as a problem.
pub const NOT_FOR_ACCOUNT: &str = "Not for this account: ";
/// The most a document of a settings-like kind (profile, preferences, server, key) may weigh.
const MAX_SMALL_DOC: usize = 64 * 1024;
/// How many profiles, MCP servers and keys one side keeps (a document that would go past this is refused).
const MAX_PROFILES: usize = 100;
const MAX_MCP: usize = 50;
/// "The time is not known" (a key or server that was there before times were kept): any real change is newer, and a copy that only
/// one side has is still copied. Such a time is never moved by a clock difference.
pub const UNKNOWN_TIME: u64 = 1;
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

/// When each item here last changed, and the tombstones (every kind).
pub fn local_side() -> Side {
    local_side_of(&KINDS)
}

thread_local! {
    /// Set while the sync itself changes a key: that must not count as the person's own change (no new time, no tombstone).
    static APPLYING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn applying<T>(f: impl FnOnce() -> T) -> T {
    let before = APPLYING.with(|a| a.replace(true));
    let r = f();
    APPLYING.with(|a| a.set(before));
    r
}

fn mcp_here() -> bool {
    !crate::tenant::is_restricted()
}

fn is_simple_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The id of a remote MCP server in the sync: its name as a tool name.
fn mcp_id(name: &str) -> String {
    mcp_client::clean_name(name)
}

/// When each item of these kinds last changed here, and the tombstones.
fn local_side_of(kinds: &[&str]) -> Side {
    let mut items = Times::new();
    let want = |k: &str| kinds.contains(&k);
    if want("chats") {
        for c in chats::load() {
            items.insert(key("chats", &c.id), c.updated);
        }
    }
    if want("memory") {
        let m = memory::load();
        for i in &m.items {
            items.insert(key("memory", &i.id), i.updated);
        }
        if m.state_updated > 0 {
            items.insert(key("memory", STATE_ID), m.state_updated);
        }
    }
    if want("personalities") {
        for p in personality::load().items {
            items.insert(key("personalities", &p.id), personality::made_at(&p));
        }
    }
    if want("profiles") {
        for p in profiles::load() {
            items.insert(key("profiles", &p.id), p.updated.max(UNKNOWN_TIME));
        }
    }
    if want("settings") {
        let u = ui_prefs::load().updated;
        if u > 0 {
            items.insert(key("settings", SETTINGS_ID), u);
        }
    }
    if want("mcp") && mcp_here() {
        for s in mcp_client::load_servers().iter().filter(|s| mcp_client::is_url(&s.command)) {
            items.insert(key("mcp", &mcp_id(&s.name)), s.updated.max(UNKNOWN_TIME));
        }
    }
    if want("keys") {
        let times = key_times();
        for id in key_candidates() {
            if crate::ai::keys::get(&id).is_some() {
                items.insert(key("keys", &id), times.get(&id).copied().unwrap_or(UNKNOWN_TIME));
            }
        }
    }
    let mut deleted = tombstones();
    deleted.retain(|k, _| split_key(k).is_some_and(|(kind, _)| want(kind) && (kind != "mcp" || mcp_here())));
    Side { items, deleted }
}

/// The one item of the `settings` kind.
pub const SETTINGS_ID: &str = "main";

/// The manifest the server sends: `{chats:{id:updated}, memory:{..}, ..., deleted:{"chats:id":at}}`.
pub fn manifest_json() -> Value {
    side_to_json(&local_side())
}

pub fn side_to_json(s: &Side) -> Value {
    // `now` is this computer's clock: the other side uses it to see how far its own clock is off
    let mut out = json!({"deleted": s.deleted, "now": chats::now_millis()});
    for kind in KINDS {
        out[kind] = json!({});
    }
    for (k, u) in &s.items {
        if let Some((kind, id)) = split_key(k) {
            out[kind][id] = json!(u);
        }
    }
    out
}

/// Reads a manifest. Anything that does not look like one is an error (a broken answer must never look like an empty server).
/// A kind that an older server does not know is simply empty.
pub fn side_from_json(v: &Value) -> Result<Side, String> {
    let bad = || "The server's answer is not a sync manifest.".to_string();
    let mut s = Side::default();
    for kind in KINDS {
        let Some(o) = v.get(kind) else {
            if OLD_KINDS.contains(&kind) {
                return Err(bad());
            }
            continue;
        };
        for (id, u) in o.as_object().ok_or_else(bad)? {
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
    // ("." and ".." are refused by the database, "/" would be lossy, and "__" is how a long document is cut into parts).
    // A key's id (`provider` or `provider@profile`) is a name in the key store, not a document, and may have the `@`.
    let extra = |c: char| kind == "keys" && c == '@';
    if crate::tenant::multi_tenant() && (id.len() > 100 || id.contains("__") || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || extra(c))) {
        return Err("That id is not allowed (letters, digits, - and _ only, at most 100 characters, no double underscore).".into());
    }
    if kind == "mcp" && !mcp_here() {
        return Err(format!("{NOT_FOR_ACCOUNT}MCP servers are not available to this account."));
    }
    Ok(kind)
}

// ---- keys: which API keys can travel, and when each was last changed here ----

/// Only a provider's own key (`openrouter`) or a profile's (`openrouter@p1700000000000`) may travel: never the Claude Code or
/// SSH entries, the sign-in credential or an MCP token.
pub fn syncable_key_id(id: &str) -> bool {
    let (prov, prof) = match id.split_once('@') {
        Some((a, b)) => (a, Some(b)),
        None => (id, None),
    };
    providers::find(prov).is_some_and(|p| p.protocol != providers::Protocol::ClaudeCode) && prof.map_or(true, is_simple_id)
}

fn key_times_path() -> std::path::PathBuf {
    crate::audit::config_dir().join("keys-times.json")
}

/// When each syncable key was last set here (only times, never a key).
fn key_times() -> Times {
    std::fs::read_to_string(key_times_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn key_times_edit(f: impl FnOnce(&mut Times)) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut t = key_times();
    f(&mut t);
    let p = key_times_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
    if let Ok(s) = serde_json::to_string(&t) {
        let tmp = p.with_extension("json.tmp");
        if crate::tenant::fits(&p, s.len()) && std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// Called by the key store when the person saves or clears a key: the key gets the time now, or leaves a tombstone.
pub(crate) fn key_touched(id: &str, cleared: bool) {
    if APPLYING.with(|a| a.get()) || !syncable_key_id(id) {
        return;
    }
    let before = key_times().get(id).copied().unwrap_or(UNKNOWN_TIME);
    if cleared {
        key_times_edit(|t| {
            t.remove(id);
        });
        tombstone("keys", id, before);
    } else {
        key_times_edit(|t| {
            t.insert(id.to_string(), chats::now_millis().max(before.saturating_add(1)));
        });
    }
}

/// Every id a key could be stored under here: each provider, and each profile's own.
fn key_candidates() -> Vec<String> {
    let mut v: Vec<String> = providers::all().iter().filter(|p| p.protocol != providers::Protocol::ClaudeCode).map(|p| p.id.to_string()).collect();
    for p in profiles::load() {
        if is_simple_id(&p.id) && providers::find(&p.provider).is_some_and(|pr| pr.protocol != providers::Protocol::ClaudeCode) {
            v.push(format!("{}@{}", p.provider, p.id));
        }
    }
    v
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
        "profiles" => profiles::load().into_iter().find(|p| p.id == id).map(|mut p| {
            p.updated = p.updated.max(UNKNOWN_TIME);
            serde_json::to_value(p).unwrap_or(Value::Null)
        }),
        "settings" if id == SETTINGS_ID => {
            let p = ui_prefs::load();
            (p.updated > 0).then(|| ui_prefs::doc(&p))
        }
        "mcp" if mcp_here() => mcp_client::load_servers()
            .into_iter()
            .find(|s| mcp_client::is_url(&s.command) && mcp_id(&s.name) == id)
            .map(|s| json!({"name": s.name, "address": s.command, "enabled": s.enabled, "updated": s.updated.max(UNKNOWN_TIME)})),
        "keys" if syncable_key_id(id) => crate::ai::keys::get(id).map(|k| json!({"id": id, "key": k, "updated": key_times().get(id).copied().unwrap_or(UNKNOWN_TIME)})),
        _ => None,
    }
}

fn local_updated(kind: &str, id: &str) -> Option<u64> {
    if kind == "keys" {
        return crate::ai::keys::get(id).map(|_| key_times().get(id).copied().unwrap_or(UNKNOWN_TIME));
    }
    local_side_of(&[kind]).items.get(&key(kind, id)).copied()
}

/// A model server address in a profile: http(s), no spaces, at most 500 characters; an account without a computer of its own (a
/// restricted one) may only name a public https address. Empty is fine (the provider's own).
fn check_profile_address(base_url: &str) -> Result<(), String> {
    if base_url.trim().is_empty() {
        return Ok(());
    }
    crate::ai::probe::check_address(base_url)?;
    if crate::tenant::is_restricted() {
        crate::ai::agent::check_restricted_provider(providers::find("custom").ok_or("no custom provider")?, base_url).map_err(|e| format!("{NOT_FOR_ACCOUNT}{e}"))?;
    }
    Ok(())
}

/// Checks a profile that came from the other side. Nothing in it is a key.
fn check_profile(id: &str, p: &mut profiles::Profile) -> Result<(), String> {
    let text_ok = |t: &str, max: usize| t.chars().count() <= max && !t.chars().any(char::is_control);
    if p.id != id || !is_simple_id(id) {
        return Err("The profile's id is not allowed.".into());
    }
    p.name = p.name.trim().to_string();
    if p.name.is_empty() || !text_ok(&p.name, 100) || !text_ok(&p.model, 300) || p.effort.as_deref().is_some_and(|e| !text_ok(e, 20)) {
        return Err("The profile has a name, model or effort that is not allowed.".into());
    }
    let prov = providers::find(&p.provider).ok_or("The profile names a provider this version does not know.")?;
    if prov.protocol == providers::Protocol::ClaudeCode && crate::tenant::is_restricted() {
        return Err(format!("{NOT_FOR_ACCOUNT}Claude Code is not available to this account."));
    }
    if p.pinned.len() > 200 || p.pinned.iter().any(|m| m.is_empty() || !text_ok(m, 300)) {
        return Err("The profile's pinned models are not valid.".into());
    }
    if let Some(u) = p.base_url.as_deref() {
        check_profile_address(u)?;
        p.base_url = Some(u.trim().to_string()).filter(|u| !u.is_empty());
    }
    Ok(())
}

/// Applies a document that came from the other side, only if it is newer than what is here (and than a tombstone).
/// Returns whether it was applied. A document that does not fit its kind is an error.
pub fn put_doc(kind: &str, id: &str, doc: &Value) -> Result<bool, String> {
    checked(kind, id)?;
    let bad = |e: serde_json::Error| format!("The {kind} document is not valid: {e}");
    if kind != "chats" && kind != "memory" && kind != "personalities" && doc.to_string().len() > MAX_SMALL_DOC {
        return Err(format!("That {kind} document is too big."));
    }
    enum Parsed {
        Chat(chats::Chat),
        State(bool, usize),
        Note(memory::Item),
        Person(personality::Personality),
        Profile(profiles::Profile),
        Settings(ui_prefs::Prefs),
        Mcp(mcp_client::ServerConfig),
        Key(String),
    }
    let (up, parsed) = match kind {
        "chats" => {
            let mut c: chats::Chat = serde_json::from_value(doc.clone()).map_err(bad)?;
            if crate::tenant::is_restricted() {
                // pictures are files on this computer's disk: a chat that came from a client names none of them
                crate::ai::agent::strip_images(&mut c.history);
            }
            // the web page draws what a chat says about attachments: only plain names and real small previews get through
            c.clean_attach();
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
        "profiles" => {
            let mut p: profiles::Profile = serde_json::from_value(doc.clone()).map_err(bad)?;
            check_profile(id, &mut p)?;
            (p.updated.max(UNKNOWN_TIME), Parsed::Profile(p))
        }
        "settings" => {
            if id != SETTINGS_ID {
                return Err("Unknown kind of item.".into());
            }
            let p: ui_prefs::Prefs = serde_json::from_value(doc.clone()).map_err(bad)?;
            let p = ui_prefs::clean(p)?;
            if p.updated == 0 {
                return Err("The preferences have no time.".into());
            }
            (p.updated, Parsed::Settings(p))
        }
        "mcp" => {
            let g = |k: &str| doc.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
            let (name, address) = (g("name"), g("address"));
            if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) || mcp_id(&name) != id {
                return Err("The server's name is not valid.".into());
            }
            // a remote server by address only: no program to start, no account or token inside the address
            crate::ai::probe::check_address(&address)?;
            let authority = address.split("://").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("");
            if !mcp_client::is_url(&address) || authority.contains('@') {
                return Err("A synced MCP server must be a web address without a user name or password.".into());
            }
            let up = doc.get("updated").and_then(Value::as_u64).filter(|u| *u > 0).ok_or("The server has no time.")?;
            let enabled = doc.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            (up, Parsed::Mcp(mcp_client::ServerConfig { name, command: address, enabled, updated: up, ..Default::default() }))
        }
        "keys" => {
            if !syncable_key_id(id) {
                return Err("That key cannot be synced.".into());
            }
            let k = doc.get("key").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if k.is_empty() || k.len() > 4096 || k.chars().any(char::is_control) {
                return Err("The key is not valid.".into());
            }
            let up = doc.get("updated").and_then(Value::as_u64).filter(|u| *u > 0).ok_or("The key has no time.")?;
            (up, Parsed::Key(k))
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
        Parsed::Profile(p) => p.id.as_str(),
        Parsed::State(..) | Parsed::Settings(_) | Parsed::Mcp(_) | Parsed::Key(_) => id,
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
        Parsed::Profile(mut p) => {
            let mut all = profiles::load();
            if !all.iter().any(|x| x.id == p.id) && all.len() >= MAX_PROFILES {
                return Err(format!("There are already {MAX_PROFILES} profiles here."));
            }
            // names are unique, ignoring case: a clash with another profile gets a number
            let base = p.name.clone();
            let mut n = 1;
            while all.iter().any(|x| x.id != p.id && x.name.eq_ignore_ascii_case(&p.name)) {
                n += 1;
                p.name = format!("{base} ({n})");
            }
            all.retain(|x| x.id != p.id);
            all.push(p);
            profiles::save_as_is(&all);
            true
        }
        Parsed::Settings(p) => ui_prefs::save_as_is(&p).map(|_| true)?,
        Parsed::Mcp(mut s) => {
            let mut all = mcp_client::load_servers();
            if let Some(x) = all.iter().find(|x| mcp_id(&x.name) == id) {
                if !mcp_client::is_url(&x.command) {
                    return Err("A program with that name is already set up here.".into());
                }
                // the headers and sign-in details of this computer stay; only the address, the on/off switch and the time come in
                let keep = x.clone();
                s = mcp_client::ServerConfig { command: s.command, enabled: s.enabled, updated: s.updated, name: keep.name.clone(), ..keep };
                all.retain(|x| mcp_id(&x.name) != id);
            } else if all.iter().filter(|x| mcp_client::is_url(&x.command)).count() >= MAX_MCP {
                return Err(format!("There are already {MAX_MCP} MCP servers here."));
            }
            all.push(s);
            mcp_client::save_servers_as_is(&all);
            true
        }
        Parsed::Key(k) => {
            applying(|| crate::ai::keys::set(id, &k))?;
            key_times_edit(|t| {
                t.insert(id.to_string(), up);
            });
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
    if kind == "settings" {
        return Err("The preferences cannot be deleted.".into());
    }
    if kind == "keys" && !syncable_key_id(id) {
        return Err("That key cannot be synced.".into());
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
        "profiles" => {
            let mut all = profiles::load();
            if let Some(p) = all.iter().find(|p| p.id == id).cloned() {
                all.retain(|x| x.id != id);
                profiles::save_as_is(&all);
                // the profile's own key goes with it (it is a separate item when keys are synced)
                applying(|| crate::ai::keys::clear(&format!("{}@{}", p.provider, p.id)));
                let mut sel = crate::ai::Selection::load();
                if sel.profile.as_deref() == Some(id) {
                    sel.profile = None;
                    sel.save();
                }
            }
        }
        "mcp" => {
            let mut all = mcp_client::load_servers();
            all.retain(|s| !(mcp_client::is_url(&s.command) && mcp_id(&s.name) == id));
            mcp_client::save_servers_as_is(&all);
        }
        "keys" => {
            applying(|| crate::ai::keys::clear(id));
            key_times_edit(|t| {
                t.remove(id);
            });
        }
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
    /// Sync the profiles (with their pinned models and addresses) and the picker preferences. Default on.
    pub profiles: bool,
    /// Sync the remote MCP servers (addresses only). Default on.
    pub mcp: bool,
    /// Also sync API keys (they then also live, encrypted, on the server). Default OFF.
    pub keys: bool,
}

fn config_path() -> std::path::PathBuf {
    crate::audit::config_dir().join("sync.json")
}

pub fn config() -> Config {
    let v: Value = std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let flag = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
    Config { url: text("url"), enabled: flag("enabled", false), email: text("email"), profiles: flag("profiles", true), mcp: flag("mcp", true), keys: flag("keys", false) }
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
    // empty = the default server (see `login::server_url`)
    let url = if url.trim().is_empty() { String::new() } else { clean_url(url)? };
    write_config(&url, enabled, &config().email)
}

/// Writes sync.json as it is (the caller has checked the address). The credential is never in this file.
pub(crate) fn write_config(url: &str, enabled: bool, email: &str) -> Result<(), String> {
    let p = config_path();
    let dir = p.parent().unwrap_or(&p);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    let c = config();
    std::fs::write(&tmp, json!({"url": url, "enabled": enabled, "email": email, "profiles": c.profiles, "mcp": c.mcp, "keys": c.keys}).to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// Chooses what else is synced besides chats, memory and personalities (the Account and sync page). Keys are off unless asked for.
pub fn set_kinds(profiles: bool, mcp: bool, keys: bool) -> Result<(), String> {
    let c = config();
    let p = config_path();
    std::fs::create_dir_all(p.parent().unwrap_or(&p)).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, json!({"url": c.url, "enabled": c.enabled, "email": c.email, "profiles": profiles, "mcp": mcp, "keys": keys}).to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    kick();
    Ok(())
}

/// The kinds this computer syncs with the current settings.
pub fn enabled_kinds(c: &Config) -> Vec<&'static str> {
    KINDS.iter().copied().filter(|k| match *k {
        "profiles" | "settings" => c.profiles,
        "mcp" => c.mcp,
        "keys" => c.keys,
        _ => true,
    }).collect()
}

/// Keeps only the items and tombstones of these kinds.
fn only_kinds(mut s: Side, kinds: &[&str]) -> Side {
    // a kind nobody knows stays (the round reports it); a known kind that is switched off goes
    let keep = |k: &String| split_key(k).map_or(true, |(kind, _)| kinds.contains(&kind));
    s.items.retain(|k, _| keep(k));
    s.deleted.retain(|k, _| keep(k));
    s
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
    load_credential(TOKEN_ID)
}

pub fn set_token(token: &str) -> Result<(), String> {
    store_credential(TOKEN_ID, token)
}

/// A device credential lives in the key store. Where there is none (a computer without secret-tool or a session bus, a server
/// shell) it is kept in a private file (mode 0600) in the config folder instead, so signing in still works there.
fn fallback_path() -> std::path::PathBuf {
    crate::audit::config_dir().join("credentials.json")
}

fn local_fallback_allowed() -> bool {
    !crate::tenant::multi_tenant() && crate::tenant::current().is_none()
}

pub(crate) fn load_credential(id: &str) -> Option<String> {
    if let Some(k) = crate::ai::keys::get(id) {
        return Some(k);
    }
    if !local_fallback_allowed() {
        return None;
    }
    let v: Value = std::fs::read_to_string(fallback_path()).ok().and_then(|s| serde_json::from_str(&s).ok())?;
    v.get(id).and_then(Value::as_str).filter(|k| !k.is_empty()).map(str::to_string)
}

pub(crate) fn store_credential(id: &str, cred: &str) -> Result<(), String> {
    let e = match crate::ai::keys::set(id, cred) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    if !local_fallback_allowed() {
        return Err(e);
    }
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::Write;
    let p = fallback_path();
    let mut m: serde_json::Map<String, Value> = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    m.insert(id.to_string(), Value::String(cred.trim().to_string()));
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e2| format!("{e} ({e2})"))?;
    f.write_all(Value::Object(m).to_string().as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

pub(crate) fn forget_credential(id: &str) {
    crate::ai::keys::clear(id);
    if !local_fallback_allowed() {
        return;
    }
    let p = fallback_path();
    if let Some(mut m) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<serde_json::Map<String, Value>>(&s).ok()) {
        m.remove(id);
        if m.is_empty() {
            let _ = std::fs::remove_file(&p);
        } else {
            let _ = std::fs::write(&p, Value::Object(m).to_string());
        }
    }
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
    if t <= UNKNOWN_TIME {
        return t;
    }
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
    let kinds = enabled_kinds(&config());
    let remote = only_kinds(side_from_json(&manifest)?, &kinds);
    // the two computers' clocks may differ: times are compared on the server's clock, and documents are moved onto it or back
    let skew = manifest.get("now").and_then(Value::as_u64).map_or(0, |n| skew_ms(sent, received, n));
    let local_raw = local_side_of(&kinds);
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
        // a rule of the account (or a server that does not know this kind yet) is not a problem to show every round
        if e.starts_with(NOT_FOR_ACCOUNT) || e == "Unknown kind of item." {
            return Ok(());
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
    // the server no longer knows this credential (removed in the web app): forget the sign-in here, so the person sees the
    // signed-out screen and nothing is sent any more
    if matches!(&result, Err(e) if e == TOKEN_REFUSED) {
        crate::login::clear_login();
    }
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
            // a setup from before sign-in existed (pasted token, no account) is switched off: nothing is sent for it
            crate::login::drop_legacy_setup();
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
                let c = config();
                if c.enabled && !c.email.is_empty() && token().is_some() {
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
        assert_eq!(split_key("bogus:a"), None);
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
        // keys go to a plain file in the sandbox (no keyring), never to the real one
        std::env::set_var("ZO_AI_KEYS_FILE", dir.join("keys.json"));
        (g, dir)
    }

    fn chat(id: &str, title: &str, updated: u64) -> Value {
        json!({"id": id, "title": title, "updated": updated, "entries": [{"kind": "msg-user", "text": "hi"}]})
    }

    #[test]
    fn attachment_facts_in_a_synced_chat_are_cleaned_and_survive_a_round_trip_and_old_chats_still_load() {
        let (_g, dir) = sandbox("attach");
        let png = crate::attach::testkit::PNG_1X1_B64;
        let good = format!("data:image/png;base64,{png}");
        let mut d = chat("c1", "with pictures", 100);
        d["entries"] = json!([
            {"kind": "msg-user", "text": "look", "attach": [
                {"kind": "image", "name": "ok.png", "size": 70, "thumb": good},
                {"kind": "image", "name": "<img src=x onerror=alert(1)>/../a\u{0}b.png", "size": 1, "thumb": "javascript:alert(1)"},
                {"kind": "image", "name": "x", "size": 1, "thumb": "data:text/html;base64,PHNjcmlwdD4="},
                {"kind": "file", "name": "gone"},
                {"kind": "text", "name": "notes.txt", "size": 9, "thumb": good}
            ]},
            {"kind": "msg-ai", "text": "a picture"}
        ]);
        assert!(put_doc("chats", "c1", &d).unwrap());
        let c = chats::load().into_iter().next().unwrap();
        let a = &c.entries[0].attach;
        assert_eq!(a.len(), 4, "the unknown kind is dropped");
        assert_eq!((a[0].name.as_str(), a[0].thumb.as_str()), ("ok.png", good.as_str()));
        assert!(!a[1].name.contains(['<', '>', '/', '\u{0}']) && a[1].thumb.is_empty());
        assert!(a[2].thumb.is_empty(), "a data address that is not a picture is not kept");
        assert_eq!((a[3].kind.as_str(), a[3].thumb.as_str()), ("text", ""), "a file has no preview picture");
        // what is stored is what is synced out again, and a chat without the field loads as before
        let out = serde_json::to_value(&c).unwrap();
        assert_eq!(out["entries"][0]["attach"][0]["name"], "ok.png");
        assert!(out["entries"][1].get("attach").is_none());
        let old: chats::Chat = serde_json::from_value(chat("c2", "old", 5)).unwrap();
        assert!(old.entries[0].attach.is_empty());
        let _ = std::fs::remove_dir_all(dir);
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
        assert!(put_doc("bogus", "x", &json!({})).is_err());
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
        let e = |k: &str, t: &str| chats::Entry { kind: k.into(), text: t.into(), ..Default::default() };
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

    // ---- profiles, settings, MCP servers and keys ----

    #[test]
    fn the_new_kinds_plan_like_the_others() {
        // a profile only here is copied, one deleted on the server is deleted here, the newest edit of an MCP server wins
        let p = plan(
            &side(&[("profiles:p1", 5), ("mcp:gh", 9), ("settings:main", 3), ("keys:openrouter@p1", 4)], &[]),
            &side(&[("profiles:p2", 6), ("mcp:gh", 7), ("settings:main", 8)], &[("keys:openrouter@p1", 10)]),
        );
        assert_eq!(names(&p.push), ["mcp:gh", "profiles:p1"]);
        assert_eq!(names(&p.pull), ["profiles:p2", "settings:main"]);
        assert_eq!(p.delete_local, [("keys:openrouter@p1".to_string(), 10)]);
        // keys use ids with an @, and every new kind is a known kind
        assert_eq!(split_key("keys:openrouter@p1"), Some(("keys", "openrouter@p1")));
        for k in ["profiles:a", "settings:main", "mcp:x", "keys:y"] {
            assert!(split_key(k).is_some(), "{k}");
        }
        assert!(split_key("secrets:a").is_none());
    }

    #[test]
    fn a_manifest_carries_the_new_kinds_and_an_old_servers_manifest_still_reads() {
        let s = side(&[("profiles:p1", 5), ("mcp:gh", 9), ("settings:main", 3), ("keys:openai", 4), ("chats:c", 1)], &[("mcp:old", 2)]);
        let back = side_from_json(&side_to_json(&s)).unwrap();
        assert_eq!(back, s);
        // a server from before these kinds has only the first three: the others are empty, not an error
        let old = json!({"chats": {"c": 1}, "memory": {}, "personalities": {}, "deleted": {}});
        assert_eq!(side_from_json(&old).unwrap().items.len(), 1);
        // but the first three are required: a broken answer is never an empty server
        assert!(side_from_json(&json!({"chats": {}, "memory": {}, "deleted": {}})).is_err());
        // an unknown time is never moved by a clock difference
        assert_eq!(shift_time(UNKNOWN_TIME, 90_000), UNKNOWN_TIME);
        assert_eq!(shift_time(1000, 5000), 6000);
    }

    #[test]
    fn only_the_chosen_kinds_travel_and_keys_are_off_by_default() {
        let (_g, dir) = sandbox("kinds");
        let c = config();
        assert!(c.profiles && c.mcp && !c.keys, "profiles and MCP on, keys off");
        assert_eq!(enabled_kinds(&c), ["chats", "memory", "personalities", "profiles", "settings", "mcp"]);
        set_kinds(true, false, true).unwrap();
        assert_eq!(enabled_kinds(&config()), ["chats", "memory", "personalities", "profiles", "settings", "keys"]);
        // writing the sign-in does not forget the choices
        write_config("", true, "a@x.test").unwrap();
        let c = config();
        assert!(c.enabled && c.keys && !c.mcp && c.email == "a@x.test");
        let s = only_kinds(side(&[("chats:a", 1), ("keys:openai", 2), ("mcp:x", 3)], &[("keys:b", 4), ("profiles:p", 5)]), &enabled_kinds(&Config { keys: false, mcp: false, ..c }));
        assert_eq!(s.items.keys().collect::<Vec<_>>(), ["chats:a"]);
        assert_eq!(s.deleted.keys().collect::<Vec<_>>(), ["profiles:p"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_profile_travels_with_its_pins_and_address_but_never_with_a_key() {
        let (_g, dir) = sandbox("prof");
        crate::ai::keys::set("openrouter", "sk-or-very-secret-1").unwrap();
        let mut sel = crate::ai::Selection { provider: "openrouter".into(), model: "x/y".into(), ..Default::default() };
        sel.base_urls.insert("openrouter".into(), "https://proxy.example.com/v1".into());
        let p = profiles::create("Work", &sel).unwrap();
        profiles::pin(&p.id, "x/y");
        profiles::pin(&p.id, "x/z");
        assert!(crate::ai::keys::get(&format!("openrouter@{}", p.id)).is_some(), "the profile has its own copy of the key here");
        let doc = get_doc("profiles", &p.id).unwrap();
        assert_eq!((doc["base_url"].as_str(), doc["pinned"].as_array().unwrap().len()), (Some("https://proxy.example.com/v1"), 2));
        assert!(doc["updated"].as_u64().unwrap() > 1);
        // no key anywhere in the profile document, the manifest or the profiles file
        let everything = format!("{doc}{}{}", manifest_json(), std::fs::read_to_string(crate::audit::config_dir().join("profiles.json")).unwrap());
        assert!(!everything.contains("sk-or-very-secret"), "{everything}");
        assert!(local_side_of(&["profiles"]).items.contains_key(&format!("profiles:{}", p.id)));
        // the other side's newer edit comes in; an older one does not
        let mut newer = doc.clone();
        newer["updated"] = json!(doc["updated"].as_u64().unwrap() + 50);
        newer["pinned"] = json!(["a/b"]);
        newer["key"] = json!("sk-smuggled");
        assert!(put_doc("profiles", &p.id, &newer).unwrap());
        assert_eq!(profiles::load()[0].pinned, ["a/b"]);
        assert!(!std::fs::read_to_string(crate::audit::config_dir().join("profiles.json")).unwrap().contains("smuggled"), "unknown fields are dropped");
        assert!(!put_doc("profiles", &p.id, &doc).unwrap());
        // a new profile from the other side, a clash of names gets a number, wrong documents are refused
        let other = json!({"id": "p5", "name": "work", "provider": "gemini", "model": "gemini-2.5-pro", "pinned": [], "updated": 77});
        assert!(put_doc("profiles", "p5", &other).unwrap());
        assert!(profiles::load().iter().any(|x| x.id == "p5" && x.name == "work (2)"));
        for bad in [
            json!({"id": "p6", "name": "x", "provider": "nonsense", "model": "m", "updated": 5}),
            json!({"id": "p6", "name": "x", "provider": "gemini", "model": "m", "base_url": "ftp://x", "updated": 5}),
            json!({"id": "p7", "name": "x", "provider": "gemini", "model": "m", "updated": 5}),
            json!({"id": "p6", "name": "", "provider": "gemini", "model": "m", "updated": 5}),
        ] {
            assert!(put_doc("profiles", "p6", &bad).is_err(), "{bad}");
        }
        // deleting here leaves a tombstone; a deletion from the other side removes the profile and unchooses it
        let mut now_sel = crate::ai::Selection::default();
        now_sel.apply_profile(&profiles::load().iter().find(|x| x.id == "p5").unwrap().clone());
        now_sel.save();
        assert!(apply_delete("profiles", "p5", chats::now_millis() + 1000).unwrap());
        assert!(profiles::load().iter().all(|x| x.id != "p5") && crate::ai::Selection::load().profile.is_none());
        profiles::delete(&p.id);
        assert!(tombstones().contains_key(&format!("profiles:{}", p.id)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_picker_preferences_travel_as_one_item() {
        let (_g, dir) = sandbox("prefs");
        assert!(get_doc("settings", SETTINGS_ID).is_none() && !local_side_of(&["settings"]).items.contains_key("settings:main"));
        ui_prefs::set_hidden(vec!["groq".into(), "groq".into(), "mistral".into()]).unwrap();
        let doc = get_doc("settings", SETTINGS_ID).unwrap();
        assert_eq!(doc["hidden_providers"], json!(["groq", "mistral"]));
        let mut newer = doc.clone();
        newer["updated"] = json!(doc["updated"].as_u64().unwrap() + 10);
        newer["hidden_providers"] = json!(["openai"]);
        assert!(put_doc("settings", SETTINGS_ID, &newer).unwrap());
        assert_eq!(ui_prefs::load().hidden_providers, ["openai"]);
        assert!(!put_doc("settings", SETTINGS_ID, &doc).unwrap());
        assert!(put_doc("settings", "other", &newer).is_err());
        assert!(apply_delete("settings", SETTINGS_ID, 5).is_err());
        let mut bad = newer.clone();
        bad["updated"] = json!(chats::now_millis() + 1000);
        bad["hidden_providers"] = json!(["has space"]);
        assert!(put_doc("settings", SETTINGS_ID, &bad).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_remote_mcp_servers_travel_by_address_and_never_with_a_token() {
        let (_g, dir) = sandbox("mcp");
        mcp_client::save_servers(&[
            mcp_client::ServerConfig { name: "github".into(), command: "https://mcp.example.com/mcp".into(), enabled: true, headers: [("Authorization".to_string(), "Bearer ghp_secret_token".to_string())].into(), client_secret: Some("shhh".into()), ..Default::default() },
            mcp_client::ServerConfig { name: "files".into(), command: "npx -y some-server /home/me".into(), enabled: true, env: [("KEY".to_string(), "env-secret".to_string())].into(), ..Default::default() },
        ]);
        let side = local_side_of(&["mcp"]);
        assert_eq!(side.items.keys().collect::<Vec<_>>(), ["mcp:github"], "a program is never listed");
        let doc = get_doc("mcp", "github").unwrap();
        assert_eq!(doc["address"], "https://mcp.example.com/mcp");
        let shown = format!("{doc}{}", manifest_json());
        for secret in ["ghp_secret", "shhh", "env-secret", "npx"] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        assert!(get_doc("mcp", "files").is_none());
        // the other side's newer edit changes the address and the switch, and the headers of this computer stay
        let mut newer = doc.clone();
        newer["updated"] = json!(doc["updated"].as_u64().unwrap() + 20);
        newer["enabled"] = json!(false);
        newer["address"] = json!("https://mcp2.example.com/mcp");
        assert!(put_doc("mcp", "github", &newer).unwrap());
        let now = mcp_client::load_servers();
        let g = now.iter().find(|s| s.name == "github").unwrap();
        assert!(!g.enabled && g.command == "https://mcp2.example.com/mcp" && g.headers.contains_key("Authorization"));
        // a new one comes in; wrong ones are refused (a program, a user name in the address, a name that is not the id)
        let fresh = json!({"name": "docs", "address": "https://docs.example.com/mcp", "enabled": true, "updated": 5});
        assert!(put_doc("mcp", "docs", &fresh).unwrap());
        for (id, d) in [
            ("run", json!({"name": "run", "address": "npx -y thing", "updated": 5})),
            ("u", json!({"name": "u", "address": "https://me:pw@x.example.com/mcp", "updated": 5})),
            ("a", json!({"name": "b", "address": "https://x.example.com/mcp", "updated": 5})),
            ("files", json!({"name": "files", "address": "https://x.example.com/mcp", "updated": 5})),
        ] {
            assert!(put_doc("mcp", id, &d).is_err(), "{d}");
        }
        // removing one here leaves a tombstone, a remote deletion removes it here (the program stays)
        mcp_client::save_servers(&mcp_client::load_servers().into_iter().filter(|s| s.name != "docs").collect::<Vec<_>>());
        assert!(tombstones().contains_key("mcp:docs"));
        assert!(apply_delete("mcp", "github", chats::now_millis() + 1000).unwrap());
        assert_eq!(mcp_client::load_servers().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["files"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keys_travel_only_as_their_own_kind_and_only_provider_keys() {
        let (_g, dir) = sandbox("keys");
        crate::ai::keys::set("openrouter", "sk-or-1").unwrap();
        crate::ai::keys::set("sync-token", "zd_not_a_provider_key_at_all").unwrap();
        crate::ai::keys::set("claude-code", "nope").unwrap();
        crate::ai::keys::set("mcp-github", "ghp_x").unwrap();
        let side = local_side_of(&["keys"]);
        assert_eq!(side.items.keys().collect::<Vec<_>>(), ["keys:openrouter"], "{:?}", side.items);
        assert!(side.items["keys:openrouter"] > 1, "saving a key gives it a time");
        assert!(get_doc("keys", "sync-token").is_none() && get_doc("keys", "claude-code").is_none() && get_doc("keys", "mcp-github").is_none());
        assert_eq!(get_doc("keys", "openrouter").unwrap()["key"], "sk-or-1");
        // the other side's newer key replaces it, without leaving a tombstone or a time of "now"
        let t = side.items["keys:openrouter"] + 100;
        assert!(put_doc("keys", "openrouter", &json!({"id": "openrouter", "key": "sk-or-2", "updated": t})).unwrap());
        assert_eq!(crate::ai::keys::get("openrouter").unwrap(), "sk-or-2");
        assert_eq!(local_side_of(&["keys"]).items["keys:openrouter"], t);
        assert!(!put_doc("keys", "openrouter", &json!({"id": "openrouter", "key": "old", "updated": t - 1})).unwrap());
        for bad in [("sync-token", json!({"key": "x", "updated": 5})), ("openrouter", json!({"key": "", "updated": t + 1})), ("openrouter", json!({"key": "a\nb", "updated": t + 1})), ("nonsense", json!({"key": "x", "updated": 5}))] {
            assert!(put_doc("keys", bad.0, &bad.1).is_err(), "{:?}", bad);
        }
        // a key for a profile
        assert!(put_doc("keys", "gemini@p123", &json!({"key": "g-1", "updated": 9})).unwrap());
        assert_eq!(crate::ai::keys::get("gemini@p123").unwrap(), "g-1");
        // clearing a key here leaves a tombstone; the other side's deletion clears it without a new one
        crate::ai::keys::clear("openrouter");
        assert!(tombstones().contains_key("keys:openrouter"));
        let tomb = tombstones()["keys:openrouter"];
        assert!(!put_doc("keys", "openrouter", &json!({"key": "back", "updated": tomb - 1})).unwrap());
        assert!(!apply_delete("keys", "gemini@p123", 5).unwrap(), "changed after that deletion");
        assert!(apply_delete("keys", "gemini@p123", chats::now_millis() + 1000).unwrap());
        assert!(crate::ai::keys::get("gemini@p123").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_restricted_account_takes_no_mcp_server_no_private_address_and_nothing_big() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let t = crate::tenant::testkit::fresh(true);
        let admin = crate::tenant::testkit::fresh(false);
        let mcp = json!({"name": "docs", "address": "https://docs.example.com/mcp", "enabled": true, "updated": 5});
        crate::tenant::scope(&t, || {
            let e = put_doc("mcp", "docs", &mcp).unwrap_err();
            assert!(e.starts_with(NOT_FOR_ACCOUNT), "{e}");
            assert!(apply_delete("mcp", "docs", 5).unwrap_err().starts_with(NOT_FOR_ACCOUNT));
            assert!(get_doc("mcp", "docs").is_none());
            assert!(manifest_json()["mcp"].as_object().unwrap().is_empty());
            // a model server on this network or without https is refused, a public https one is taken
            let prof = |url: &str, id: &str| json!({"id": id, "name": id, "provider": "custom", "model": "m", "base_url": url, "updated": 5});
            for bad in ["http://127.0.0.1:8000/v1", "https://10.0.0.5/v1", "http://api.example.com/v1", "https://localhost/v1", "https://user@api.example.com/v1"] {
                let e = put_doc("profiles", "pa", &prof(bad, "pa")).unwrap_err();
                assert!(e.starts_with(NOT_FOR_ACCOUNT), "{bad}: {e}");
            }
            assert!(put_doc("profiles", "pb", &prof("https://8.8.8.8/v1", "pb")).unwrap());
            // Claude Code is not for it, a document over the limit is refused
            let cc = json!({"id": "pc", "name": "pc", "provider": "claude-code", "model": "sonnet", "updated": 5});
            assert!(put_doc("profiles", "pc", &cc).unwrap_err().starts_with(NOT_FOR_ACCOUNT));
            let big = json!({"id": "pd", "name": "pd", "provider": "gemini", "model": "m", "updated": 5, "pad": "x".repeat(70_000)});
            assert!(put_doc("profiles", "pd", &big).unwrap_err().contains("too big"));
            let many: Vec<String> = (0..300).map(|i| format!("model-{i}")).collect();
            let lots = json!({"id": "pe", "name": "pe", "provider": "gemini", "model": "m", "updated": 5, "pinned": many});
            assert!(put_doc("profiles", "pe", &lots).is_err());
            // its API keys may travel (they are its own, in its own encrypted store)
            assert!(put_doc("keys", "gemini", &json!({"key": "g-k", "updated": 5})).unwrap());
            assert!(crate::ai::keys::get("gemini").is_some());
        });
        // the same documents are fine for a person with a computer of their own (not restricted)
        crate::tenant::scope(&admin, || {
            assert!(put_doc("mcp", "docs", &mcp).unwrap());
            assert!(put_doc("profiles", "pa", &json!({"id": "pa", "name": "pa", "provider": "custom", "model": "m", "base_url": "http://10.0.0.5:8000/v1", "updated": 5})).unwrap());
            assert!(manifest_json()["mcp"]["docs"].is_u64());
        });
    }
}
