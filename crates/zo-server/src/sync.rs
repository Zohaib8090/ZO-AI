//! Keeps ZO-AI's files and Firestore the same. The server's disk is lost on every redeploy, so at start everything is fetched
//! from Firestore, and after that every change on disk is sent there within a few seconds.
//!
//! Layout under the root document: `chats/{chat id}` (one chat each), `memory/{note id}` (one note each), `files/{name}` (the
//! small files: the chosen model, profiles, personalities, projects, ...; `c_` is the config folder, `d_` the data folder).
//! API keys are sent encrypted (`files/keys_enc`), and only when ZO_SERVER_SECRET is set.

use crate::firestore::{doc_id, encode_parts, Firestore, Joined};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

type Name = (String, String);

pub struct Sync {
    pub fs: Firestore,
    pub secret: Option<String>,
    pub keys_file: PathBuf,
    /// What Firestore holds, as far as we know: a fingerprint of the text and how many documents it takes.
    known: BTreeMap<Name, (u64, usize)>,
    /// Documents Firestore holds that could not be read (parts missing, or not valid): left exactly as they are, never deleted.
    /// The number is how many documents they take, so that a later save of the same name cleans up after itself.
    damaged: BTreeMap<Name, usize>,
    seen: String,
}

fn hash(s: &str) -> u64 {
    let d = Sha256::digest(s.as_bytes());
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

fn cipher(secret: &str) -> Aes256Gcm {
    Aes256Gcm::new_from_slice(&Sha256::digest(secret.as_bytes())).expect("32 bytes")
}

pub fn encrypt(secret: &str, text: &str) -> Result<String, String> {
    let mut nonce = [0u8; 12];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut nonce)).map_err(|e| e.to_string())?;
    let mut out = nonce.to_vec();
    out.extend(cipher(secret).encrypt(Nonce::from_slice(&nonce), text.as_bytes()).map_err(|_| "could not encrypt")?);
    Ok(base64::engine::general_purpose::STANDARD.encode(out))
}

pub fn decrypt(secret: &str, data: &str) -> Result<String, String> {
    let raw = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|_| "the stored keys are damaged")?;
    if raw.len() < 13 {
        return Err("the stored keys are damaged".into());
    }
    let plain = cipher(secret).decrypt(Nonce::from_slice(&raw[..12]), &raw[12..]).map_err(|_| "the stored keys cannot be opened: ZO_SERVER_SECRET is not the one they were saved with")?;
    String::from_utf8(plain).map_err(|_| "the stored keys are damaged".into())
}

/// The small files worth keeping (not the big lists, the log, sockets or folders).
fn small_files(dir: &Path, skip: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() || meta.len() > 4_000_000 || p == skip || ["chats.json", "memory.json", "audit.jsonl"].contains(&name.as_str()) || name.ends_with(".tmp") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.push((name, text));
        }
    }
    out
}

/// What should be in Firestore, from what is on disk. The bool marks a text that must be encrypted before it is sent.
pub fn desired(config: &Path, data: &Path, keys_file: &Path, with_keys: bool) -> BTreeMap<Name, (String, bool)> {
    let mut out = BTreeMap::new();
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    if let Some(Value::Array(chats)) = read(data.join("chats.json")) {
        for c in chats {
            if let Some(id) = c.get("id").and_then(Value::as_str).filter(|i| !i.is_empty()) {
                out.insert(("chats".into(), doc_id(id)), (c.to_string(), false));
            }
        }
    }
    if let Some(Value::Object(mut mem)) = read(data.join("memory.json")) {
        if let Some(Value::Array(items)) = mem.remove("items") {
            for i in items {
                if let Some(id) = i.get("id").and_then(Value::as_str).filter(|i| !i.is_empty()) {
                    out.insert(("memory".into(), doc_id(id)), (i.to_string(), false));
                }
            }
        }
        out.insert(("files".into(), "memory_state".into()), (Value::Object(mem).to_string(), false));
    }
    for (name, text) in small_files(config, keys_file) {
        out.insert(("files".into(), doc_id(&format!("c_{name}"))), (text, false));
    }
    for (name, text) in small_files(data, keys_file) {
        out.insert(("files".into(), doc_id(&format!("d_{name}"))), (text, false));
    }
    if with_keys {
        if let Ok(text) = std::fs::read_to_string(keys_file) {
            out.insert(("files".into(), "keys_enc".into()), (text, true));
        }
    }
    out
}

fn write(p: &Path, text: &str) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, text);
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
    pub fn new(fs: Firestore, secret: Option<String>, keys_file: PathBuf) -> Sync {
        Sync { fs, secret, keys_file, known: BTreeMap::new(), damaged: BTreeMap::new(), seen: String::new() }
    }

    /// At start: bring everything from Firestore to the disk. Returns how many chats and notes came.
    pub fn pull(&mut self, config: &Path, data: &Path) -> Result<(usize, usize), String> {
        let chats = self.fs.all("chats")?;
        let memory = self.fs.all("memory")?;
        let files = self.fs.all("files")?;
        // a chat or note that cannot be read (damaged parts, or not valid JSON) is kept in Firestore as it is and reported; it is
        // not on disk, so it is never saved over or deleted by this program
        let mut bad: Vec<Name> = Vec::new();
        let (chat_docs, note_docs) = (readable("chats", &chats, &mut bad), readable("memory", &memory, &mut bad));
        for key in &bad {
            let parts = (if key.0 == "chats" { &chats } else { &memory }).get(&key.1).map_or(1, |j| j.parts);
            eprintln!("zo-server: {}/{} in Firestore cannot be read; it is left as it is", key.0, key.1);
            self.damaged.insert(key.clone(), parts);
        }
        if !chats.is_empty() {
            write(&data.join("chats.json"), &Value::Array(chat_docs).to_string());
        }
        let mut state = files.get("memory_state").filter(|j| j.ok).and_then(|j| serde_json::from_str::<Value>(&j.text).ok()).unwrap_or_else(|| json!({}));
        if !memory.is_empty() || files.contains_key("memory_state") {
            if let Some(o) = state.as_object_mut() {
                o.insert("items".into(), Value::Array(note_docs));
            }
            write(&data.join("memory.json"), &state.to_string());
        }
        for (name, j) in &files {
            let text = &j.text;
            if !j.ok {
                eprintln!("zo-server: files/{name} in Firestore cannot be read; it is left as it is");
                self.damaged.insert(("files".into(), name.clone()), j.parts);
                continue;
            }
            if let Some(n) = name.strip_prefix("c_") {
                write(&config.join(n), text);
            } else if let Some(n) = name.strip_prefix("d_") {
                write(&data.join(n), text);
            } else if name == "keys_enc" {
                match self.secret.as_deref().map(|s| decrypt(s, text)) {
                    Some(Ok(plain)) => write(&self.keys_file, &plain),
                    Some(Err(e)) => eprintln!("zo-server: {e}"),
                    None => eprintln!("zo-server: saved API keys were found but ZO_SERVER_SECRET is not set, so they stay closed"),
                }
            }
        }
        // remember what is there, so that only real changes are sent later
        let now = desired(config, data, &self.keys_file, self.secret.is_some());
        for (col, m) in [("chats", &chats), ("memory", &memory), ("files", &files)] {
            for (name, j) in m {
                let key = (col.to_string(), name.clone());
                if self.damaged.contains_key(&key) {
                    continue; // not known: so it is neither compared nor deleted
                }
                let h = now.get(&key).map(|(t, _)| hash(t)).unwrap_or(0);
                self.known.insert(key, (h, j.parts));
            }
        }
        Ok((chats.len() - bad.iter().filter(|k| k.0 == "chats").count(), memory.len() - bad.iter().filter(|k| k.0 == "memory").count()))
    }

    /// Sends what changed on disk. Returns how many documents were written or removed.
    pub fn push(&mut self, config: &Path, data: &Path) -> Result<usize, String> {
        let print = fingerprint(&[config, data]);
        if print == self.seen {
            return Ok(0);
        }
        let want = desired(config, data, &self.keys_file, self.secret.is_some());
        let mut n = 0;
        for (key, (text, secret)) in &want {
            let h = hash(text);
            let old = self.known.get(key).copied();
            if old.map(|o| o.0) == Some(h) {
                continue;
            }
            let body = if *secret { encrypt(self.secret.as_deref().unwrap_or(""), text)? } else { text.clone() };
            let old_parts = old.map(|o| o.1).or_else(|| self.damaged.get(key).copied()).unwrap_or(0);
            match self.fs.put(&key.0, &key.1, &body, old_parts) {
                Ok(parts) => {
                    self.known.insert(key.clone(), (h, parts));
                    self.damaged.remove(key);
                    n += 1;
                }
                Err(e) => {
                    // some parts may have been written: the next save (maybe a shorter one) has to remove what is left over
                    let may_take = old_parts.max(encode_parts(&body).len());
                    self.known.insert(key.clone(), (old.map_or(0, |o| o.0), may_take));
                    return Err(e);
                }
            }
        }
        let gone: Vec<Name> = self.known.keys().filter(|k| !want.contains_key(*k)).cloned().collect();
        for key in gone {
            let parts = self.known.get(&key).map(|o| o.1).unwrap_or(1);
            self.fs.delete(&key.0, &key.1, parts)?;
            self.known.remove(&key);
            n += 1;
        }
        self.seen = print;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_survive_encryption_and_a_wrong_secret_fails() {
        let c = encrypt("s3cret", "{\"openrouter\":\"sk-abc\"}").unwrap();
        assert!(!c.contains("sk-abc"));
        assert_eq!(decrypt("s3cret", &c).unwrap(), "{\"openrouter\":\"sk-abc\"}");
        assert!(decrypt("other", &c).is_err());
    }

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

    #[test]
    fn disk_becomes_documents() {
        let dir = std::env::temp_dir().join(format!("zo-sync-{}", std::process::id()));
        let (config, data) = (dir.join("c"), dir.join("d"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("chats.json"), r#"[{"id":"c1","title":"A"},{"id":"c2","title":"B"}]"#).unwrap();
        std::fs::write(data.join("memory.json"), r#"{"enabled":true,"every":3,"read":{"c1":2},"items":[{"id":"m1","text":"x"}]}"#).unwrap();
        std::fs::write(config.join("ai.json"), "{}").unwrap();
        std::fs::write(data.join("audit.jsonl"), "log").unwrap();
        let keys = config.join("keys.json");
        std::fs::write(&keys, r#"{"openrouter":"sk"}"#).unwrap();
        let d = desired(&config, &data, &keys, true);
        assert!(d.contains_key(&("chats".into(), "c1".into())) && d.contains_key(&("chats".into(), "c2".into())));
        assert!(d.contains_key(&("memory".into(), "m1".into())));
        assert!(d[&("files".into(), "memory_state".into())].0.contains("\"every\":3"));
        assert!(d.contains_key(&("files".into(), "c_ai.json".into())));
        assert!(!d.keys().any(|k| k.1.contains("audit")), "the log is not sent");
        assert!(!d.contains_key(&("files".into(), "c_keys.json".into())), "keys never go in plain text");
        assert!(d[&("files".into(), "keys_enc".into())].1, "keys are marked for encryption");
        assert!(!desired(&config, &data, &keys, false).contains_key(&("files".into(), "keys_enc".into())));
        let _ = std::fs::remove_dir_all(dir);
    }
}
