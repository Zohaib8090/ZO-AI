//! Keeps one person's ZO-AI files and their Firestore documents the same. The server's disk is lost on every redeploy and a person's
//! files are taken off the disk when they have been idle for a while, so a person's data is fetched from Firestore the first time they
//! are seen, and after that every change on disk is sent there within a few seconds.
//!
//! Layout under the person's root document `users/<tenant id>`: `chats/{chat id}` (one chat each), `memory/{note id}` (one note each),
//! `files/{name}` (the small files: the chosen model, profiles, personalities, ...; `c_` is the config folder, `d_` the data folder).
//! API keys are kept as the encrypted file `keys.enc` (see zo-core's `ai::keys`) and travel as `files/keys_enc` in base64: Firestore
//! only ever holds the ciphertext, which only this server's secret opens.

use crate::db::{all_texts, delete_text, put_text, Db};
use crate::firestore::{doc_id, encode_parts, Joined};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

type Name = (String, String);

pub struct Sync {
    db: Arc<dyn Db>,
    /// `users/<tenant id>`
    root: String,
    tenant: String,
    secret: String,
    config: PathBuf,
    data: PathBuf,
    /// What Firestore holds, as far as we know: a fingerprint of the text and how many documents it takes.
    known: BTreeMap<Name, (u64, usize)>,
    /// Documents Firestore holds that could not be read (parts missing, or not valid): left exactly as they are, never deleted.
    /// The number is how many documents they take, so that a later save of the same name cleans up after itself.
    damaged: BTreeMap<Name, usize>,
    seen: String,
    /// The person's data has been fetched (nothing is sent before that: an empty start must never overwrite what is saved).
    pub loaded: bool,
    /// The account was deleted: nothing is ever sent again.
    pub dead: bool,
}

fn hash(s: &str) -> u64 {
    let d = Sha256::digest(s.as_bytes());
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

/// The small files worth keeping (not the big lists, the log, sockets or folders).
fn small_files(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() || meta.len() > 4_000_000 || ["chats.json", "memory.json", "audit.jsonl", "keys.enc", "keys.json", "sync.json"].contains(&name.as_str()) || name.ends_with(".tmp") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.push((name, text));
        }
    }
    out
}

/// What should be in Firestore, from what is on disk.
pub fn desired(config: &Path, data: &Path) -> BTreeMap<Name, String> {
    let mut out = BTreeMap::new();
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    if let Some(Value::Array(chats)) = read(data.join("chats.json")) {
        for c in chats {
            if let Some(id) = c.get("id").and_then(Value::as_str).filter(|i| !i.is_empty()) {
                out.insert(("chats".into(), doc_id(id)), c.to_string());
            }
        }
    }
    if let Some(Value::Object(mut mem)) = read(data.join("memory.json")) {
        if let Some(Value::Array(items)) = mem.remove("items") {
            for i in items {
                if let Some(id) = i.get("id").and_then(Value::as_str).filter(|i| !i.is_empty()) {
                    out.insert(("memory".into(), doc_id(id)), i.to_string());
                }
            }
        }
        out.insert(("files".into(), "memory_state".into()), Value::Object(mem).to_string());
    }
    for (name, text) in small_files(config) {
        out.insert(("files".into(), doc_id(&format!("c_{name}"))), text);
    }
    for (name, text) in small_files(data) {
        out.insert(("files".into(), doc_id(&format!("d_{name}"))), text);
    }
    // the keys leave the disk only as the ciphertext they already are
    if let Ok(raw) = std::fs::read(config.join(zo_core::ai::keys::ENC_FILE)) {
        out.insert(("files".into(), "keys_enc".into()), base64::engine::general_purpose::STANDARD.encode(raw));
    }
    out
}

fn write(p: &Path, bytes: impl AsRef<[u8]>) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p) {
        let _ = f.write_all(bytes.as_ref());
    }
}

/// Something that changes whenever a file in the two folders changes.
fn fingerprint(dirs: &[&Path]) -> String {
    let mut s = String::new();
    for d in dirs {
        let mut rows: Vec<String> = std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file().then(|| format!("{}:{}:{:?};", e.file_name().to_string_lossy(), m.len(), m.modified().ok()))
            })
            .collect();
        rows.sort();
        s.extend(rows);
    }
    s
}

/// The documents of one collection that can be read (parts complete, valid JSON); the names of the others go to `bad`.
fn readable(col: &str, m: &BTreeMap<String, Joined>, bad: &mut Vec<Name>) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, j) in m {
        match j.ok.then(|| serde_json::from_str::<Value>(&j.text).ok()).flatten() {
            Some(v) => out.push(v),
            None => bad.push((col.to_string(), name.clone())),
        }
    }
    out
}

impl Sync {
    pub fn new(db: Arc<dyn Db>, secret: String, tenant: &str, config: PathBuf, data: PathBuf) -> Sync {
        Sync { db, root: format!("users/{tenant}"), tenant: tenant.to_string(), secret, config, data, known: BTreeMap::new(), damaged: BTreeMap::new(), seen: String::new(), loaded: false, dead: false }
    }

    fn col(&self, name: &str) -> String {
        format!("{}/{name}", self.root)
    }

    /// The first time: bring everything from Firestore to the disk. Returns how many chats and notes came.
    pub fn pull(&mut self) -> Result<(usize, usize), String> {
        if self.dead {
            return Err("This account was deleted.".into());
        }
        let (config, data) = (self.config.clone(), self.data.clone());
        let chats = all_texts(&*self.db, &self.col("chats"))?;
        let memory = all_texts(&*self.db, &self.col("memory"))?;
        let files = all_texts(&*self.db, &self.col("files"))?;
        // a chat or note that cannot be read (damaged parts, or not valid JSON) is kept in Firestore as it is; it is not on disk,
        // so it is never saved over or deleted by this program
        let mut bad: Vec<Name> = Vec::new();
        let (chat_docs, note_docs) = (readable("chats", &chats, &mut bad), readable("memory", &memory, &mut bad));
        for key in &bad {
            let parts = (if key.0 == "chats" { &chats } else { &memory }).get(&key.1).map_or(1, |j| j.parts);
            self.damaged.insert(key.clone(), parts);
        }
        if !bad.is_empty() {
            eprintln!("zo-server: {} documents of one account cannot be read; they are left as they are", bad.len());
        }
        if !chats.is_empty() {
            write(&data.join("chats.json"), Value::Array(chat_docs).to_string());
        }
        let mut state = files.get("memory_state").filter(|j| j.ok).and_then(|j| serde_json::from_str::<Value>(&j.text).ok()).unwrap_or_else(|| json!({}));
        if !memory.is_empty() || files.contains_key("memory_state") {
            if let Some(o) = state.as_object_mut() {
                o.insert("items".into(), Value::Array(note_docs));
            }
            write(&data.join("memory.json"), state.to_string());
        }
        for (name, j) in &files {
            let text = &j.text;
            if !j.ok {
                self.damaged.insert(("files".into(), name.clone()), j.parts);
                continue;
            }
            if let Some(n) = name.strip_prefix("c_") {
                if safe_name(n) {
                    write(&config.join(n), text);
                }
            } else if let Some(n) = name.strip_prefix("d_") {
                if safe_name(n) {
                    write(&data.join(n), text);
                }
            } else if name == "keys_enc" {
                // only keys this server can open are put on disk; anything else stays in Firestore untouched
                let raw = base64::engine::general_purpose::STANDARD.decode(text.trim()).unwrap_or_default();
                if zo_core::ai::keys::decrypt_map(&self.secret, &self.tenant, &raw).is_ok() {
                    write(&config.join(zo_core::ai::keys::ENC_FILE), raw);
                } else {
                    eprintln!("zo-server: the saved keys of one account cannot be opened (is ZO_SERVER_SECRET the one they were saved with?); they stay closed");
                    self.damaged.insert(("files".into(), name.clone()), j.parts);
                }
            }
        }
        // remember what is there, so that only real changes are sent later
        let now = desired(&config, &data);
        for (col, m) in [("chats", &chats), ("memory", &memory), ("files", &files)] {
            for (name, j) in m {
                let key = (col.to_string(), name.clone());
                if self.damaged.contains_key(&key) {
                    continue; // not known: so it is neither compared nor deleted
                }
                let h = now.get(&key).map(|t| hash(t)).unwrap_or(0);
                self.known.insert(key, (h, j.parts));
            }
        }
        self.loaded = true;
        Ok((chats.len() - bad.iter().filter(|k| k.0 == "chats").count(), memory.len() - bad.iter().filter(|k| k.0 == "memory").count()))
    }

    /// Is everything on disk in Firestore already? (Used before a person's files are taken off the disk.)
    pub fn is_clean(&self) -> bool {
        self.dead || fingerprint(&[&self.config, &self.data]) == self.seen
    }

    /// Sends what changed on disk. Returns how many documents were written or removed. Does nothing before the first pull and after
    /// the account was deleted.
    pub fn push(&mut self) -> Result<usize, String> {
        if !self.loaded || self.dead {
            return Ok(0);
        }
        let (config, data) = (self.config.clone(), self.data.clone());
        let print = fingerprint(&[&config, &data]);
        if print == self.seen {
            return Ok(0);
        }
        let want = desired(&config, &data);
        let mut n = 0;
        // one document that cannot be saved must not keep the others from being saved: go on, and report the first failure at the end
        let mut first_err: Option<String> = None;
        for (key, text) in &want {
            let h = hash(text);
            let old = self.known.get(key).copied();
            if old.map(|o| o.0) == Some(h) {
                continue;
            }
            let old_parts = old.map(|o| o.1).or_else(|| self.damaged.get(key).copied()).unwrap_or(0);
            match put_text(&*self.db, &self.col(&key.0), &key.1, text, old_parts) {
                Ok(parts) => {
                    self.known.insert(key.clone(), (h, parts));
                    self.damaged.remove(key);
                    n += 1;
                }
                Err(e) => {
                    // some parts may have been written: the next save (maybe a shorter one) has to remove what is left over
                    let may_take = old_parts.max(encode_parts(text).len());
                    self.known.insert(key.clone(), (old.map_or(0, |o| o.0), may_take));
                    first_err.get_or_insert(e);
                }
            }
        }
        let gone: Vec<Name> = self.known.keys().filter(|k| !want.contains_key(*k)).cloned().collect();
        for key in gone {
            let parts = self.known.get(&key).map(|o| o.1).unwrap_or(1);
            match delete_text(&*self.db, &self.col(&key.0), &key.1, parts) {
                Ok(()) => {
                    self.known.remove(&key);
                    n += 1;
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        self.seen = print;
        Ok(n)
    }
}

/// A file name that stays inside its folder.
fn safe_name(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains('/') && !n.contains('\\') && !n.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemDb;

    #[test]
    fn a_document_that_cannot_be_read_is_reported_and_left_out_not_dropped_silently() {
        let mut m = BTreeMap::new();
        let j = |text: &str, ok: bool| Joined { text: text.into(), parts: 1, ok };
        m.insert("good".to_string(), j("{\"id\":\"good\"}", true));
        m.insert("cut".to_string(), j("", false));
        m.insert("garbled".to_string(), j("{\"id\":", true));
        let mut bad = Vec::new();
        let docs = readable("chats", &m, &mut bad);
        assert_eq!(docs.len(), 1);
        assert_eq!(bad, vec![("chats".to_string(), "cut".to_string()), ("chats".to_string(), "garbled".to_string())]);
    }

    fn dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("zo-sync-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (config, data) = (dir.join("c"), dir.join("d"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        (dir, config, data)
    }

    #[test]
    fn disk_becomes_documents() {
        let (dir, config, data) = dirs("desired");
        std::fs::write(data.join("chats.json"), r#"[{"id":"c1","title":"A"},{"id":"c2","title":"B"}]"#).unwrap();
        std::fs::write(data.join("memory.json"), r#"{"enabled":true,"every":3,"read":{"c1":2},"items":[{"id":"m1","text":"x"}]}"#).unwrap();
        std::fs::write(config.join("ai.json"), "{}").unwrap();
        std::fs::write(data.join("audit.jsonl"), "log").unwrap();
        std::fs::write(config.join("keys.json"), r#"{"openrouter":"sk-plain"}"#).unwrap();
        std::fs::write(config.join("keys.enc"), [1u8, 2, 3, 255, 0]).unwrap();
        let d = desired(&config, &data);
        assert!(d.contains_key(&("chats".into(), "c1".into())) && d.contains_key(&("chats".into(), "c2".into())));
        assert!(d.contains_key(&("memory".into(), "m1".into())));
        assert!(d[&("files".into(), "memory_state".into())].contains("\"every\":3"));
        assert!(d.contains_key(&("files".into(), "c_ai.json".into())));
        assert!(!d.keys().any(|k| k.1.contains("audit")), "the log is not sent");
        assert!(!d.contains_key(&("files".into(), "c_keys.json".into())), "plain keys never go anywhere");
        assert!(!d.contains_key(&("files".into(), "c_keys.enc".into())));
        assert_eq!(d[&("files".into(), "keys_enc".into())], base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3, 255, 0]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_sent_before_the_first_pull_and_a_dead_account_sends_nothing() {
        let (dir, config, data) = dirs("guard");
        let db = Arc::new(MemDb::default());
        let mut s = Sync::new(db.clone(), "sec".into(), &"a".repeat(32), config.clone(), data.clone());
        std::fs::write(data.join("chats.json"), r#"[{"id":"c1","title":"A"}]"#).unwrap();
        assert_eq!(s.push().unwrap(), 0, "not loaded yet");
        assert_eq!(db.count_under("users"), 0);
        s.pull().unwrap();
        assert!(s.push().unwrap() > 0);
        assert!(db.count_under(&format!("users/{}", "a".repeat(32))) > 0);
        let before = db.count_under("users");
        s.dead = true;
        std::fs::write(data.join("chats.json"), r#"[{"id":"c1","title":"A"},{"id":"c2","title":"B"}]"#).unwrap();
        assert_eq!(s.push().unwrap(), 0);
        assert_eq!(db.count_under("users"), before, "a deleted account is never written again");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keys_travel_only_as_ciphertext_and_only_keys_this_server_can_open_are_put_on_disk() {
        let (dir, config, data) = dirs("keys");
        let tenant = "b".repeat(32);
        let db = Arc::new(MemDb::default());
        let mut keys = serde_json::Map::new();
        keys.insert("openrouter".into(), json!("sk-secret-value"));
        let raw = zo_core::ai::keys::encrypt_map("sec", &tenant, &keys).unwrap();
        std::fs::write(config.join("keys.enc"), &raw).unwrap();
        let mut s = Sync::new(db.clone(), "sec".into(), &tenant, config.clone(), data.clone());
        s.pull().unwrap();
        s.push().unwrap();
        let stored = db.list_docs(&format!("users/{tenant}/files")).unwrap();
        let all: String = stored.values().cloned().collect();
        assert!(!all.contains("sk-secret-value"), "Firestore never sees a key");
        // a new disk (a redeploy) gets the same keys back
        let (dir2, config2, data2) = dirs("keys2");
        let mut s2 = Sync::new(db.clone(), "sec".into(), &tenant, config2.clone(), data2);
        s2.pull().unwrap();
        assert_eq!(std::fs::read(config2.join("keys.enc")).unwrap(), raw);
        // another secret cannot open them: not written, left in Firestore
        let (dir3, config3, data3) = dirs("keys3");
        let mut s3 = Sync::new(db.clone(), "other secret".into(), &tenant, config3.clone(), data3);
        s3.pull().unwrap();
        assert!(!config3.join("keys.enc").exists());
        assert!(db.get_doc(&format!("users/{tenant}/files"), "keys_enc").unwrap().is_some());
        for d in [dir, dir2, dir3] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn pulled_names_cannot_leave_the_folder() {
        for bad in ["", ".", "..", "a/b", "..\\x"] {
            assert!(!safe_name(bad), "{bad}");
        }
        assert!(safe_name("ai.json"));
    }
}
