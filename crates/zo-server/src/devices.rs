//! Device tokens: what the desktop app's sync and the `zo` terminal client use instead of a Google sign-in. A person makes one on
//! their Account page. It is `zd_` plus 32 random bytes (base64url), shown once; only its sha256 is stored: in the person's own
//! list (`users/<tenant>/meta/devices`, to show and revoke it) and in the index `deviceidx/<sha256>` (to find the person from the
//! token). Scope `sync` reaches only `/api/sync/*`; `full` also `/api/op`, `/api/send` and `/api/reply`. A token is never logged.

use crate::db::Db;
use crate::tenants::Bucket;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const INDEX: &str = "deviceidx";
pub const MAX_DEVICES: usize = 10;
const PREFIX: &str = "zd_";
/// How long a token that was found (or not found) is remembered before Firestore is asked again. A revoke clears it at once.
const KNOWN_FOR: Duration = Duration::from_secs(300);
const UNKNOWN_FOR: Duration = Duration::from_secs(60);
/// Lookups of unknown tokens that may reach Firestore per minute, for the whole server (guessing tokens cannot cost unlimited reads).
const LOOKUPS_PER_MIN: u32 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Sync,
    Full,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Scope> {
        match s {
            "sync" => Some(Scope::Sync),
            "full" => Some(Scope::Full),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Scope::Sync => "sync",
            Scope::Full => "full",
        }
    }
}

/// One entry of a person's own list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub scope: Scope,
    pub created: u64,
    pub last_used: u64,
    /// sha256 of the token, hex: the name of its index document.
    pub hash: String,
}

impl Device {
    /// What the Account page gets (no hash).
    pub fn public(&self) -> Value {
        json!({"id": self.id, "name": self.name, "scope": self.scope.name(), "created": self.created, "last_used": self.last_used})
    }
}

/// What a token stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub tenant: String,
    pub id: String,
    pub scope: Scope,
    /// Fingerprint of the owner's e-mail address (see `auth::email_hash`): the owner's admin or blocked status is looked up from it.
    pub eh: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Miss {
    /// Not a token of ours, or revoked.
    Unknown,
    /// Too many unknown tokens are being tried: try again in this many seconds.
    Rate(u64),
    Storage(String),
}

pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Looks like one of our tokens (checked before anything is looked up).
pub fn well_formed(token: &str) -> bool {
    token.len() == PREFIX.len() + 43 && token.starts_with(PREFIX) && token[PREFIX.len()..].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn random_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| format!("no random numbers: {e}"))?;
    Ok(format!("{PREFIX}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn list_collection(tenant: &str) -> String {
    format!("users/{tenant}/meta")
}

struct Cached {
    at: Instant,
    found: Option<Resolved>,
    /// When the person's list last got this token's `last_used`.
    stamped: Instant,
}

pub struct Devices {
    db: Arc<dyn Db>,
    cache: Mutex<HashMap<String, Cached>>,
    lookups: Mutex<Bucket>,
    /// Makes, lists and revokes one at a time inside this process.
    edit: Mutex<()>,
}

impl Devices {
    pub fn new(db: Arc<dyn Db>) -> Devices {
        Devices { db, cache: Mutex::new(HashMap::new()), lookups: Mutex::new(Bucket::new(LOOKUPS_PER_MIN, Instant::now())), edit: Mutex::new(()) }
    }

    fn read(&self, tenant: &str) -> Result<Vec<Device>, String> {
        match self.db.get_doc(&list_collection(tenant), "devices")? {
            None => Ok(Vec::new()),
            Some(t) if t.trim().is_empty() => Ok(Vec::new()),
            Some(t) => serde_json::from_str(&t).map_err(|_| "The list of devices cannot be read.".to_string()),
        }
    }

    fn write(&self, tenant: &str, list: &[Device]) -> Result<(), String> {
        self.db.put_doc(&list_collection(tenant), "devices", &serde_json::to_string(list).map_err(|e| e.to_string())?)
    }

    pub fn list(&self, tenant: &str) -> Result<Vec<Device>, String> {
        let _g = self.edit.lock().unwrap_or_else(|e| e.into_inner());
        self.read(tenant)
    }

    /// Makes a token. Returns it (this is the only time it exists in clear) with its list entry. `eh` is `auth::email_hash`.
    pub fn create(&self, tenant: &str, eh: &str, restricted: bool, name: &str, scope: Scope) -> Result<(String, Device), String> {
        let _g = self.edit.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = self.read(tenant)?;
        if list.len() >= MAX_DEVICES {
            return Err(format!("You have {MAX_DEVICES} devices already. Revoke one first."));
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(60).collect::<String>().trim().to_string();
        let token = random_token()?;
        let hash = hash_token(&token);
        let id = hash[..12].to_string();
        let dev = Device { id, name: if name.is_empty() { "Device".into() } else { name }, scope, created: now_secs(), last_used: 0, hash: hash.clone() };
        // the index first: a token that is in the index but not in the list is only an orphan that "delete account" sweeps away
        let entry = json!({"tenant": tenant, "id": dev.id, "scope": scope.name(), "restricted": restricted, "eh": eh});
        self.db.put_doc(INDEX, &hash, &entry.to_string())?;
        list.push(dev.clone());
        if let Err(e) = self.write(tenant, &list) {
            let _ = self.db.delete_doc(INDEX, &hash);
            return Err(e);
        }
        Ok((token, dev))
    }

    /// Revokes one device of this person. A device that is not theirs (or not there) is "not found".
    pub fn revoke(&self, tenant: &str, id: &str) -> Result<bool, String> {
        let _g = self.edit.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = self.read(tenant)?;
        let Some(pos) = list.iter().position(|d| d.id == id) else { return Ok(false) };
        let dev = list.remove(pos);
        // the index first, so the token stops working even if the list cannot be saved right now
        self.db.delete_doc(INDEX, &dev.hash)?;
        self.cache.lock().unwrap().remove(&dev.hash);
        self.write(tenant, &list)?;
        Ok(true)
    }

    /// Removes every token of one person (account deletion), including orphans: the whole index is scanned, so this finishes
    /// the job even if an earlier run stopped half way and their list is gone already. Returns how many index entries went.
    pub fn purge(&self, tenant: &str) -> Result<usize, String> {
        let _g = self.edit.lock().unwrap_or_else(|e| e.into_inner());
        let mut n = 0;
        for (hash, text) in self.db.list_docs(INDEX)? {
            let mine = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("tenant").and_then(Value::as_str).map(|t| t == tenant)).unwrap_or(false);
            if mine {
                self.db.delete_doc(INDEX, &hash)?;
                self.cache.lock().unwrap().remove(&hash);
                n += 1;
            }
        }
        self.cache.lock().unwrap().retain(|_, c| c.found.as_ref().map(|f| f.tenant != tenant).unwrap_or(true));
        Ok(n)
    }

    /// Whose token is this?
    pub fn resolve(&self, token: &str) -> Result<Resolved, Miss> {
        if !well_formed(token) {
            return Err(Miss::Unknown);
        }
        let hash = hash_token(token);
        let now = Instant::now();
        let mut stamp = false;
        {
            let mut c = self.cache.lock().unwrap();
            if let Some(e) = c.get_mut(&hash) {
                let ttl = if e.found.is_some() { KNOWN_FOR } else { UNKNOWN_FOR };
                if now.duration_since(e.at) < ttl {
                    if e.found.is_some() && now.duration_since(e.stamped) > Duration::from_secs(3600) {
                        e.stamped = now;
                        stamp = true;
                    }
                    let found = e.found.clone();
                    drop(c);
                    if stamp {
                        if let Some(f) = &found {
                            self.stamp(f, &hash);
                        }
                    }
                    return found.ok_or(Miss::Unknown);
                }
            }
        }
        if let Err(wait) = self.lookups.lock().unwrap().take(now) {
            return Err(Miss::Rate(wait));
        }
        let text = self.db.get_doc(INDEX, &hash).map_err(Miss::Storage)?;
        let found = text.and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| {
            Some(Resolved {
                tenant: v.get("tenant")?.as_str()?.to_string(),
                id: v.get("id")?.as_str()?.to_string(),
                scope: Scope::parse(v.get("scope")?.as_str()?)?,
                eh: v.get("eh").and_then(Value::as_str).unwrap_or("").to_string(),
            })
        });
        {
            let mut c = self.cache.lock().unwrap();
            if c.len() > 5000 {
                c.retain(|_, e| now.duration_since(e.at) < KNOWN_FOR);
            }
            c.insert(hash.clone(), Cached { at: now, found: found.clone(), stamped: now });
        }
        if let Some(f) = &found {
            self.stamp(f, &hash);
        }
        found.ok_or(Miss::Unknown)
    }

    /// Notes in the person's list that the token was used just now (at most once an hour per token).
    fn stamp(&self, r: &Resolved, hash: &str) {
        let _g = self.edit.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut list) = self.read(&r.tenant) {
            if let Some(d) = list.iter_mut().find(|d| d.hash == hash) {
                d.last_used = now_secs();
                let _ = self.write(&r.tenant, &list);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemDb;

    fn id(n: u8) -> String {
        format!("{n:032x}")
    }

    #[test]
    fn a_token_has_the_documented_shape_and_only_its_hash_is_stored() {
        let db = Arc::new(MemDb::default());
        let d = Devices::new(db.clone());
        let (token, dev) = d.create(&id(1), "eh1", true, "  My laptop ", Scope::Sync).unwrap();
        assert!(token.starts_with("zd_") && token.len() == 46 && well_formed(&token));
        assert_eq!(dev.name, "My laptop");
        let everything: String = db.docs.lock().unwrap().iter().map(|((c, i), t)| format!("{c}/{i}={t}")).collect();
        assert!(!everything.contains(&token), "the token itself is nowhere in the database");
        assert!(everything.contains(&hash_token(&token)));
        let idx = db.get_doc(INDEX, &hash_token(&token)).unwrap().unwrap();
        let v: Value = serde_json::from_str(&idx).unwrap();
        assert_eq!((v["tenant"].as_str().unwrap(), v["scope"].as_str().unwrap(), v["restricted"].as_bool().unwrap()), (id(1).as_str(), "sync", true));
        // two tokens are never the same
        assert_ne!(d.create(&id(1), "eh1", true, "", Scope::Full).unwrap().0, token);
    }

    #[test]
    fn a_token_finds_its_owner_a_made_up_one_finds_nobody_and_a_revoked_one_stops_working() {
        let db = Arc::new(MemDb::default());
        let d = Devices::new(db.clone());
        let (token, dev) = d.create(&id(1), "eh", true, "x", Scope::Full).unwrap();
        let r = d.resolve(&token).unwrap();
        assert_eq!((r.tenant.as_str(), r.id.as_str(), r.scope), (id(1).as_str(), dev.id.as_str(), Scope::Full));
        assert_eq!(d.resolve(&format!("zd_{}", "A".repeat(43))), Err(Miss::Unknown));
        assert_eq!(d.resolve("garbage"), Err(Miss::Unknown));
        assert_eq!(d.resolve(&token.replace("zd_", "zx_")), Err(Miss::Unknown));
        // another person cannot revoke it
        assert!(!d.revoke(&id(2), &dev.id).unwrap());
        assert!(d.resolve(&token).is_ok());
        assert!(d.revoke(&id(1), &dev.id).unwrap());
        assert_eq!(d.resolve(&token), Err(Miss::Unknown), "revoked, even though it was cached");
        assert!(d.list(&id(1)).unwrap().is_empty());
        assert!(!d.revoke(&id(1), &dev.id).unwrap(), "revoking again is harmless");
    }

    #[test]
    fn use_is_noted_and_lists_never_show_the_hash() {
        let db = Arc::new(MemDb::default());
        let d = Devices::new(db);
        let (token, _) = d.create(&id(1), "eh", false, "x", Scope::Sync).unwrap();
        d.resolve(&token).unwrap();
        let l = d.list(&id(1)).unwrap();
        assert!(l[0].last_used > 0);
        let shown = l[0].public().to_string();
        assert!(!shown.contains(&l[0].hash) && shown.contains("\"scope\":\"sync\""));
    }

    #[test]
    fn at_most_ten_devices_and_guessing_is_rate_limited() {
        let db = Arc::new(MemDb::default());
        let d = Devices::new(db);
        for _ in 0..MAX_DEVICES {
            d.create(&id(1), "eh", false, "x", Scope::Sync).unwrap();
        }
        assert!(d.create(&id(1), "eh", false, "x", Scope::Sync).is_err());
        assert!(d.create(&id(2), "eh", false, "x", Scope::Sync).is_ok(), "another person has their own ten");
        let mut limited = false;
        for n in 0..(LOOKUPS_PER_MIN as usize + 20) {
            let guess = format!("zd_{n:043}");
            if let Err(Miss::Rate(s)) = d.resolve(&guess) {
                assert!(s >= 1);
                limited = true;
            }
        }
        assert!(limited, "unknown tokens cannot reach the database without limit");
    }

    #[test]
    fn purge_removes_every_token_of_one_person_even_orphans_and_leaves_others() {
        let db = Arc::new(MemDb::default());
        let d = Devices::new(db.clone());
        let (a1, _) = d.create(&id(1), "eh", false, "a1", Scope::Sync).unwrap();
        let (a2, _) = d.create(&id(1), "eh", false, "a2", Scope::Full).unwrap();
        let (b1, _) = d.create(&id(2), "eh2", false, "b1", Scope::Full).unwrap();
        // a half-finished earlier delete: the list is gone but the index entries remain
        db.delete_doc(&list_collection(&id(1)), "devices").unwrap();
        assert_eq!(d.purge(&id(1)).unwrap(), 2);
        assert_eq!(d.resolve(&a1), Err(Miss::Unknown));
        assert_eq!(d.resolve(&a2), Err(Miss::Unknown));
        assert!(d.resolve(&b1).is_ok());
        assert_eq!(d.purge(&id(1)).unwrap(), 0);
    }
}
