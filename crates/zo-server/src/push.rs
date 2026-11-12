//! Notifications when the browser is closed: Web Push (RFC 8030 delivery, RFC 8291 payload encryption "aes128gcm", RFC 8292 VAPID)
//! for browsers and installed web apps, and Firebase Cloud Messaging (HTTP v1) for the Android app.
//!
//! What leaves the server, and to whom: only "your answer is ready" plus the chat's id (and the chat's title, cut to 60 characters,
//! only for a device whose owner switched "show the chat title" on). It goes to the push service the browser itself named when it
//! subscribed (Google, Mozilla, Apple or Microsoft: a fixed allow-list, https only, so nobody can make the server call an address
//! of their choosing) or to FCM. The payload is encrypted for the browser; the push service cannot read it. See docs/NOTIFICATIONS.md.
//!
//! The VAPID key pair is made once and kept in Firestore (`server/vapid`), encrypted with a key derived from `ZO_SERVER_SECRET`, so a
//! redeploy keeps it (a new key would silently end every subscription).

use crate::db::Db;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How many devices of one account may be registered for notifications.
pub const MAX_SUBS: usize = 5;
/// Pushes per account per hour (an answer that finishes with nobody connected is one push).
pub const PUSHES_PER_HOUR: usize = 30;
/// The longest chat title that is ever sent.
pub const TITLE_CHARS: usize = 60;
const MAX_ENDPOINT: usize = 1024;
const MAX_KEY_TEXT: usize = 200;
const MAX_TOKEN: usize = 4096;
const MAX_LABEL: usize = 60;
/// What one encrypted record can hold: 4096 - 16 (tag) - 1 (delimiter).
const MAX_PLAIN: usize = 4096 - 17;
/// Firestore: the key pair is a server-level document, not part of any person's data.
const SERVER_COLLECTION: &str = "server";
const VAPID_DOC: &str = "vapid";
/// Where a person's subscriptions live: `users/<tenant>/meta/push`.
const SUBS_DOC: &str = "push";

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("the operating system has no random numbers");
    b
}

fn new_secret() -> SecretKey {
    loop {
        if let Ok(k) = SecretKey::from_slice(&random_bytes::<32>()) {
            return k;
        }
    }
}

fn public_bytes(k: &SecretKey) -> Vec<u8> {
    k.public_key().to_encoded_point(false).as_bytes().to_vec()
}

/// base64url as browsers write it (no padding); a padded or standard-alphabet text is accepted too.
pub fn unb64(s: &str) -> Result<Vec<u8>, String> {
    if s.len() > MAX_KEY_TEXT * 2 {
        return Err("A key is too long.".into());
    }
    let t: String = s.trim().trim_end_matches('=').chars().map(|c| match c {
        '+' => '-',
        '/' => '_',
        c => c,
    }).collect();
    B64.decode(t.as_bytes()).map_err(|_| "A key is not base64.".to_string())
}

// ---------------------------------------------------------------------------------------------------------------------------
// RFC 8291: encryption of the message

/// The body of the push request: salt, record size, the sender's public key, and one encrypted record (RFC 8188 "aes128gcm").
/// `ua_public` is the browser's `p256dh` key (65 bytes), `auth` its 16-byte `auth` secret.
pub fn encrypt(ua_public: &[u8], auth: &[u8], plaintext: &[u8], as_secret: &SecretKey, salt: &[u8; 16]) -> Result<Vec<u8>, String> {
    if auth.len() != 16 {
        return Err("The auth secret must be 16 bytes.".into());
    }
    if ua_public.len() != 65 || ua_public[0] != 4 {
        return Err("The browser's public key must be 65 bytes (uncompressed).".into());
    }
    if plaintext.len() > MAX_PLAIN {
        return Err("The message is too long.".into());
    }
    let ua = PublicKey::from_sec1_bytes(ua_public).map_err(|_| "The browser's public key is not a point on the curve.".to_string())?;
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua.as_affine());
    let as_pub = public_bytes(as_secret);
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(&as_pub);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes().as_slice()).expand(&key_info, &mut ikm).map_err(|e| e.to_string())?;
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|e| e.to_string())?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|e| e.to_string())?;
    let mut record = plaintext.to_vec();
    record.push(2); // the last record
    let ct = Aes128Gcm::new_from_slice(&cek).map_err(|e| e.to_string())?.encrypt(Nonce::from_slice(&nonce), record.as_slice()).map_err(|_| "Encryption failed.".to_string())?;
    let mut out = Vec::with_capacity(21 + as_pub.len() + ct.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(as_pub.len() as u8);
    out.extend_from_slice(&as_pub);
    out.extend_from_slice(&ct);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------------------
// RFC 8292: VAPID

fn json_text(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

/// The signed token: `header.claims.signature`, ES256, claims in the order of the RFC's example (aud, exp, sub).
pub fn vapid_jwt(key: &SigningKey, aud: &str, sub: &str, exp: u64) -> String {
    let head = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = B64.encode(format!(r#"{{"aud":{},"exp":{exp},"sub":{}}}"#, json_text(aud), json_text(sub)).as_bytes());
    let input = format!("{head}.{claims}");
    let sig: Signature = key.sign(input.as_bytes());
    format!("{input}.{}", B64.encode(sig.to_bytes()))
}

/// The value of the `Authorization` header: `vapid t=<jwt>, k=<public key>`.
pub fn vapid_header(key: &SecretKey, aud: &str, sub: &str, exp: u64) -> String {
    format!("vapid t={}, k={}", vapid_jwt(&SigningKey::from(key), aud, sub, exp), B64.encode(public_bytes(key)))
}

// ---------------------------------------------------------------------------------------------------------------------------
// Where a push may go

/// The push services a subscription may point at. The browser chooses the address, so a malicious person could name any address:
/// everything not on this list is refused (https only, no port other than 443, no user name, no IP number).
#[derive(Clone, Default)]
pub struct Allow {
    /// Tests only: one more origin (for example `https://push.test`) that a fake transport answers for.
    #[cfg(test)]
    test_origin: Option<String>,
}

const EXACT_HOSTS: [&str; 3] = ["fcm.googleapis.com", "updates.push.services.mozilla.com", "web.push.apple.com"];
const SUFFIX_HOSTS: [&str; 2] = [".push.apple.com", ".notify.windows.com"];

impl Allow {
    #[cfg(test)]
    pub fn for_test(origin: &str) -> Allow {
        Allow { test_origin: Some(origin.to_string()) }
    }

    /// Ok(origin) (`https://host`) when the endpoint may be called.
    pub fn check(&self, endpoint: &str) -> Result<String, String> {
        let bad = || "That push address is not one of the allowed push services.".to_string();
        if endpoint.len() > MAX_ENDPOINT || endpoint.chars().any(|c| c <= ' ' || c == '\x7f' || c == '\\') {
            return Err(bad());
        }
        #[cfg(test)]
        if let Some(o) = &self.test_origin {
            if endpoint.starts_with(&format!("{o}/")) {
                return Ok(o.clone());
            }
        }
        let rest = endpoint.strip_prefix("https://").ok_or_else(bad)?;
        let end = rest.find(['/', '?', '#']).ok_or_else(bad)?;
        let (authority, path) = rest.split_at(end);
        if !path.starts_with('/') || authority.is_empty() || authority.contains('@') {
            return Err(bad());
        }
        let host = match authority.rsplit_once(':') {
            Some((h, "443")) => h,
            Some(_) => return Err(bad()),
            None => authority,
        }
        .to_ascii_lowercase();
        if !host.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-') || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
            return Err(bad());
        }
        if EXACT_HOSTS.contains(&host.as_str()) || SUFFIX_HOSTS.iter().any(|s| host.ends_with(s) && host.len() > s.len()) {
            Ok(format!("https://{host}"))
        } else {
            Err(bad())
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// Sending

/// How a request leaves the server (the real one is `ureq`; the tests record instead).
pub trait Transport: Send + Sync {
    fn post(&self, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<(u16, String), String>;
}

struct Net;

impl Transport for Net {
    fn post(&self, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<(u16, String), String> {
        // never follow a redirect: the address was checked, where it would lead was not
        let agent = ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(15)).build();
        let mut r = agent.post(url);
        for (k, v) in headers {
            r = r.set(k, v);
        }
        let text = |resp: ureq::Response| resp.into_string().unwrap_or_default().chars().take(2000).collect::<String>();
        match r.send_bytes(body) {
            Ok(resp) => Ok((resp.status(), text(resp))),
            Err(ureq::Error::Status(c, resp)) => Ok((c, text(resp))),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// One registered device of a person.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sub {
    pub id: String,
    /// "webpush" or "fcm"
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub p256dh: String,
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub token: String,
    /// The person allowed the chat title in the notification of this device.
    #[serde(default)]
    pub show_title: bool,
}

enum Outcome {
    Sent,
    /// The push service says the subscription is over (404, 410, an unregistered token): remove it.
    Gone,
    Failed(String),
}

/// What a notification is about.
pub struct Note<'a> {
    pub chat: &'a str,
    pub title: &'a str,
}

fn clean_chat_id(id: &str) -> String {
    if id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') { id.to_string() } else { String::new() }
}

fn clean_title(t: &str) -> String {
    let one: String = t.chars().filter(|c| !c.is_control()).collect();
    let one = one.trim();
    if one.chars().count() > TITLE_CHARS {
        let cut: String = one.chars().take(TITLE_CHARS - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        one.to_string()
    }
}

#[derive(Clone)]
struct Vapid {
    secret: SecretKey,
    public: String,
}

/// The push machinery of the server.
pub struct Hub {
    db: Option<Arc<dyn Db>>,
    secret: String,
    subject: String,
    transport: Arc<dyn Transport>,
    allow: Allow,
    vapid: Mutex<Option<Result<Vapid, String>>>,
    /// tenant -> subscriptions (read from the database once; written through). Also the only store without a database (test mode).
    subs: Mutex<HashMap<String, Vec<Sub>>>,
    /// Serialises changes of the lists.
    change: Mutex<()>,
    sent: Mutex<HashMap<String, VecDeque<Instant>>>,
    #[cfg(test)]
    fcm_fixed: Option<(String, String)>,
    /// How many pushes were handed to a push service (a counter for the tests and the log).
    pub delivered: AtomicUsize,
}

impl Default for Hub {
    fn default() -> Hub {
        Hub::new(None, "", "mailto:nobody@invalid.example".into())
    }
}

impl Hub {
    /// `subject` is the contact in the VAPID token (`mailto:` or `https://`): push services use it to reach the operator.
    pub fn new(db: Option<Arc<dyn Db>>, secret: &str, subject: String) -> Hub {
        Hub {
            db,
            secret: secret.to_string(),
            subject,
            transport: Arc::new(Net),
            allow: Allow::default(),
            vapid: Mutex::new(None),
            subs: Mutex::new(HashMap::new()),
            change: Mutex::new(()),
            sent: Mutex::new(HashMap::new()),
            #[cfg(test)]
            fcm_fixed: None,
            delivered: AtomicUsize::new(0),
        }
    }

    #[cfg(test)]
    pub fn with_transport(mut self, t: Arc<dyn Transport>) -> Hub {
        self.transport = t;
        self
    }

    #[cfg(test)]
    pub fn with_allow(mut self, a: Allow) -> Hub {
        self.allow = a;
        self
    }

    #[cfg(test)]
    pub fn with_fcm(mut self, project: &str, token: &str) -> Hub {
        self.fcm_fixed = Some((project.into(), token.into()));
        self
    }

    // ----- the VAPID key pair

    fn vapid_cipher(&self) -> Aes256Gcm {
        let key = Sha256::digest([b"zo-vapid-key".as_slice(), self.secret.as_bytes()].concat());
        Aes256Gcm::new_from_slice(&key).expect("32 bytes")
    }

    fn load_vapid(&self) -> Result<Vapid, String> {
        let make = |k: SecretKey| Vapid { public: B64.encode(public_bytes(&k)), secret: k };
        let Some(db) = &self.db else { return Ok(make(new_secret())) }; // test mode: a key for this run only
        if self.secret.is_empty() {
            return Err("ZO_SERVER_SECRET is not set, so the notification key cannot be kept safely.".into());
        }
        let cipher = self.vapid_cipher();
        if let Some(t) = db.get_doc(SERVER_COLLECTION, VAPID_DOC)? {
            let v: Value = serde_json::from_str(&t).map_err(|_| "The stored notification key cannot be read.".to_string())?;
            let part = |k: &str| v.get(k).and_then(Value::as_str).ok_or_else(|| "The stored notification key is incomplete.".to_string()).and_then(unb64);
            let (n, c) = (part("n")?, part("c")?);
            if n.len() != 12 {
                return Err("The stored notification key is damaged.".into());
            }
            // when this fails ZO_SERVER_SECRET changed: do not make a new key (every subscription would silently die); say so
            let plain = cipher.decrypt(Nonce::from_slice(&n), c.as_slice()).map_err(|_| "The stored notification key does not open with this ZO_SERVER_SECRET. Restore the old secret, or delete the document server/vapid in Firestore to start over (every device must then subscribe again).".to_string())?;
            return SecretKey::from_slice(&plain).map(make).map_err(|_| "The stored notification key is damaged.".to_string());
        }
        let k = new_secret();
        let nonce = random_bytes::<12>();
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), k.to_bytes().as_slice()).map_err(|_| "Encryption failed.".to_string())?;
        db.put_doc(SERVER_COLLECTION, VAPID_DOC, &json!({"v": 1, "n": B64.encode(nonce), "c": B64.encode(ct), "created": now_secs()}).to_string())?;
        Ok(make(k))
    }

    fn vapid(&self) -> Result<Vapid, String> {
        let mut g = self.vapid.lock().unwrap();
        if g.is_none() {
            *g = Some(self.load_vapid());
        }
        let r = g.clone().unwrap();
        if r.is_err() {
            *g = None; // try again next time (the database may have been down)
        }
        r
    }

    /// The public key a browser subscribes with.
    pub fn public_key(&self) -> Result<String, String> {
        self.vapid().map(|v| v.public)
    }

    fn fcm_access(&self) -> Option<Result<(String, String), String>> {
        #[cfg(test)]
        if let Some((p, t)) = &self.fcm_fixed {
            return Some(Ok((p.clone(), t.clone())));
        }
        self.db.as_ref()?.fcm_access()
    }

    // ----- the lists

    fn load(&self, tenant: &str) -> Result<Vec<Sub>, String> {
        if let Some(l) = self.subs.lock().unwrap().get(tenant) {
            return Ok(l.clone());
        }
        let list: Vec<Sub> = match &self.db {
            None => Vec::new(),
            Some(db) => match db.get_doc(&crate::devices::list_collection(tenant), SUBS_DOC)? {
                None => Vec::new(),
                Some(t) if t.trim().is_empty() => Vec::new(),
                Some(t) => serde_json::from_str(&t).map_err(|_| "The list of notification devices cannot be read.".to_string())?,
            },
        };
        let mut g = self.subs.lock().unwrap();
        if g.len() > 5000 {
            g.clear(); // a cache, not the record
        }
        g.insert(tenant.to_string(), list.clone());
        Ok(list)
    }

    fn store(&self, tenant: &str, list: Vec<Sub>) -> Result<(), String> {
        if let Some(db) = &self.db {
            let col = crate::devices::list_collection(tenant);
            if list.is_empty() {
                db.delete_doc(&col, SUBS_DOC)?;
            } else {
                db.put_doc(&col, SUBS_DOC, &serde_json::to_string(&list).map_err(|e| e.to_string())?)?;
            }
        }
        self.subs.lock().unwrap().insert(tenant.to_string(), list);
        Ok(())
    }

    /// Does this person have a device that can be notified? (`keep_running` is only honoured then.)
    pub fn has_any(&self, tenant: &str) -> bool {
        self.load(tenant).map(|l| !l.is_empty()).unwrap_or(false)
    }

    /// An account was deleted: forget what is remembered of it (the database side is removed with the rest of `users/<tenant>`).
    pub fn forget(&self, tenant: &str) {
        let _g = self.change.lock().unwrap();
        self.subs.lock().unwrap().remove(tenant);
        self.sent.lock().unwrap().remove(tenant);
    }

    pub fn list(&self, tenant: &str) -> Result<Vec<Value>, String> {
        Ok(self.load(tenant)?.iter().map(|s| json!({"id": s.id, "kind": s.kind, "label": s.label, "created": s.created})).collect())
    }

    pub fn subscribe(&self, tenant: &str, v: &Value) -> Result<String, (u16, String)> {
        let err = |s: u16, m: &str| (s, m.to_string());
        let kind = v.get("kind").and_then(Value::as_str).unwrap_or("");
        let label: String = v.get("label").and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control()).take(MAX_LABEL).collect();
        let show_title = v.get("show_title").and_then(Value::as_bool).unwrap_or(false);
        let mut sub = Sub { id: String::new(), kind: kind.to_string(), label: label.trim().to_string(), created: now_secs(), endpoint: String::new(), p256dh: String::new(), auth: String::new(), token: String::new(), show_title };
        match kind {
            "webpush" => {
                let s = v.get("subscription").filter(|s| s.is_object()).ok_or_else(|| err(400, "Send the browser's subscription."))?;
                let endpoint = s.get("endpoint").and_then(Value::as_str).unwrap_or("");
                self.allow.check(endpoint).map_err(|e| (400, e))?;
                let key = |k: &str| s.get("keys").and_then(|x| x.get(k)).and_then(Value::as_str).unwrap_or("");
                let p256dh = unb64(key("p256dh")).map_err(|e| (400, e))?;
                let auth = unb64(key("auth")).map_err(|e| (400, e))?;
                if auth.len() != 16 || p256dh.len() != 65 || PublicKey::from_sec1_bytes(&p256dh).is_err() {
                    return Err(err(400, "The subscription's keys are not right."));
                }
                (sub.endpoint, sub.p256dh, sub.auth) = (endpoint.to_string(), B64.encode(&p256dh), B64.encode(&auth));
            }
            "fcm" => {
                let t = v.get("token").and_then(Value::as_str).unwrap_or("");
                if self.fcm_access().is_none() {
                    return Err(err(400, "This server does not send to the Android app."));
                }
                if t.len() < 20 || t.len() > MAX_TOKEN || !t.chars().all(|c| c.is_ascii_alphanumeric() || "-_:.".contains(c)) {
                    return Err(err(400, "That is not a notification token."));
                }
                sub.token = t.to_string();
            }
            _ => return Err(err(400, "The kind must be webpush or fcm.")),
        }
        let _g = self.change.lock().unwrap();
        let mut list = self.load(tenant).map_err(|e| (503, e))?;
        // the same browser or app registering again replaces its entry
        if let Some(old) = list.iter_mut().find(|o| o.kind == sub.kind && (o.endpoint == sub.endpoint && o.token == sub.token)) {
            sub.id = old.id.clone();
            sub.created = old.created;
            *old = sub.clone();
        } else {
            if list.len() >= MAX_SUBS {
                return Err(err(409, "This account already has 5 devices for notifications. Remove one first."));
            }
            sub.id = hex(&random_bytes::<8>());
            list.push(sub.clone());
        }
        self.store(tenant, list).map_err(|e| {
            eprintln!("zo-server: {e}");
            (503, "The storage is not reachable right now. Try again in a moment.".to_string())
        })?;
        Ok(sub.id)
    }

    pub fn unsubscribe(&self, tenant: &str, id: &str) -> Result<bool, (u16, String)> {
        let _g = self.change.lock().unwrap();
        let mut list = self.load(tenant).map_err(|e| (503, e))?;
        let before = list.len();
        list.retain(|s| s.id != id);
        if list.len() == before {
            return Ok(false);
        }
        self.store(tenant, list).map_err(|e| (503, e))?;
        Ok(true)
    }

    /// /api/account/push_*: the answer for a signed-in person's own tenant.
    pub fn account_call(&self, name: &str, v: &Value, tenant: &str) -> (u16, Value) {
        let fail = |s: u16, m: &str| (s, json!({"ok": false, "error": m}));
        match name {
            "push_config" => match self.public_key() {
                Ok(k) => (200, json!({"ok": true, "vapid_public_key": k, "fcm": self.fcm_access().is_some()})),
                Err(e) => {
                    eprintln!("zo-server: notifications: {e}");
                    (200, json!({"ok": true, "vapid_public_key": "", "fcm": self.fcm_access().is_some()}))
                }
            },
            "push_subscribe" => {
                // a browser can only subscribe with a key; say so plainly if this server has none
                if v.get("kind").and_then(Value::as_str) == Some("webpush") && self.public_key().is_err() {
                    return fail(503, "Notifications when the browser is closed are not available on this server.");
                }
                match self.subscribe(tenant, v) {
                    Ok(id) => (200, json!({"ok": true, "id": id})),
                    Err((s, m)) => fail(s, &m),
                }
            }
            "push_unsubscribe" => match self.unsubscribe(tenant, v.get("id").and_then(Value::as_str).unwrap_or("")) {
                Ok(true) => (200, json!({"ok": true})),
                Ok(false) => fail(404, "There is no such device."),
                Err((s, m)) => fail(s, &m),
            },
            "push_list" => match self.list(tenant) {
                Ok(l) => (200, json!({"ok": true, "subscriptions": l})),
                Err(e) => fail(503, &e),
            },
            _ => fail(404, "Unknown account call."),
        }
    }

    // ----- sending

    /// At most `PUSHES_PER_HOUR` an hour per person.
    fn may_push(&self, tenant: &str) -> bool {
        let mut g = self.sent.lock().unwrap();
        let q = g.entry(tenant.to_string()).or_default();
        let hour = Duration::from_secs(3600);
        while q.front().is_some_and(|t| t.elapsed() > hour) {
            q.pop_front();
        }
        if q.len() >= PUSHES_PER_HOUR {
            return false;
        }
        q.push_back(Instant::now());
        true
    }

    fn payload(sub: &Sub, n: &Note) -> Vec<u8> {
        let mut p = json!({"v": 1, "type": "answer", "chat": clean_chat_id(n.chat)});
        if sub.show_title {
            let t = clean_title(n.title);
            if !t.is_empty() {
                p["title"] = json!(t);
            }
        }
        p.to_string().into_bytes()
    }

    fn send_web(&self, sub: &Sub, n: &Note) -> Outcome {
        let origin = match self.allow.check(&sub.endpoint) {
            Ok(o) => o,
            Err(e) => return Outcome::Failed(e),
        };
        let vapid = match self.vapid() {
            Ok(v) => v,
            Err(e) => return Outcome::Failed(e),
        };
        let (ua, auth) = match (unb64(&sub.p256dh), unb64(&sub.auth)) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return Outcome::Gone,
        };
        let body = match encrypt(&ua, &auth, &Self::payload(sub, n), &new_secret(), &random_bytes::<16>()) {
            Ok(b) => b,
            Err(e) => return Outcome::Failed(e),
        };
        let headers = vec![
            ("Authorization".to_string(), vapid_header(&vapid.secret, &origin, &self.subject, now_secs() + 12 * 3600)),
            ("Content-Encoding".to_string(), "aes128gcm".to_string()),
            ("Content-Type".to_string(), "application/octet-stream".to_string()),
            ("TTL".to_string(), "3600".to_string()),
            ("Urgency".to_string(), "normal".to_string()),
        ];
        match self.transport.post(&sub.endpoint, &headers, &body) {
            Ok((s, _)) if (200..300).contains(&s) => Outcome::Sent,
            Ok((404 | 410, _)) => Outcome::Gone,
            Ok((s, b)) => Outcome::Failed(format!("{origin} answered {s} {}", b.chars().take(120).collect::<String>())),
            Err(e) => Outcome::Failed(e),
        }
    }

    fn send_fcm(&self, sub: &Sub, n: &Note) -> Outcome {
        let (project, bearer) = match self.fcm_access() {
            Some(Ok(x)) => x,
            Some(Err(e)) => return Outcome::Failed(e),
            None => return Outcome::Failed("FCM is not set up.".into()),
        };
        let chat = clean_chat_id(n.chat);
        let mut data = json!({"type": "answer", "chat": chat});
        let mut body_text = "Your answer is ready".to_string();
        if sub.show_title {
            let t = clean_title(n.title);
            if !t.is_empty() {
                data["title"] = json!(t);
                body_text = t;
            }
        }
        let msg = json!({"message": {
            "token": sub.token,
            "notification": {"title": "ZO-AI", "body": body_text},
            "data": data,
            "android": {"priority": "HIGH", "notification": {"tag": format!("zo-chat-{chat}")}},
        }});
        let url = format!("https://fcm.googleapis.com/v1/projects/{project}/messages:send");
        let headers = vec![("Authorization".to_string(), format!("Bearer {bearer}")), ("Content-Type".to_string(), "application/json".to_string())];
        match self.transport.post(&url, &headers, msg.to_string().as_bytes()) {
            Ok((s, _)) if (200..300).contains(&s) => Outcome::Sent,
            Ok((s, b)) if s == 404 || b.contains("UNREGISTERED") || (s == 400 && b.contains("not a valid FCM registration token")) => Outcome::Gone,
            Ok((s, b)) => Outcome::Failed(format!("FCM answered {s} {}", b.chars().take(120).collect::<String>())),
            Err(e) => Outcome::Failed(e),
        }
    }

    /// Sends the "answer ready" notification to every device of the person. Returns how many were handed over. Subscriptions the
    /// push service says are over are removed. Runs on the caller's thread: use `notify_answer` to not wait.
    pub fn deliver_answer(&self, tenant: &str, n: &Note) -> usize {
        let list = match self.load(tenant) {
            Ok(l) if !l.is_empty() => l,
            _ => return 0,
        };
        if !self.may_push(tenant) {
            eprintln!("zo-server: notifications: the hourly limit for one account was reached");
            return 0;
        }
        let (mut sent, mut gone) = (0, Vec::new());
        for sub in &list {
            let out = if sub.kind == "fcm" { self.send_fcm(sub, n) } else { self.send_web(sub, n) };
            match out {
                Outcome::Sent => sent += 1,
                Outcome::Gone => gone.push(sub.id.clone()),
                Outcome::Failed(e) => eprintln!("zo-server: notifications: not delivered ({}): {e}", sub.kind),
            }
        }
        self.delivered.fetch_add(sent, Ordering::SeqCst);
        if !gone.is_empty() {
            let _g = self.change.lock().unwrap();
            if let Ok(mut now) = self.load(tenant) {
                now.retain(|s| !gone.contains(&s.id));
                if let Err(e) = self.store(tenant, now) {
                    eprintln!("zo-server: notifications: could not remove expired devices: {e}");
                }
            }
        }
        sent
    }

    /// Like `deliver_answer`, on a thread of its own.
    pub fn notify_answer(self: &Arc<Self>, tenant: &str, chat: &str, title: &str) {
        let (hub, tenant, chat, title) = (self.clone(), tenant.to_string(), chat.to_string(), title.to_string());
        std::thread::spawn(move || {
            hub.deliver_answer(&tenant, &Note { chat: &chat, title: &title });
        });
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
pub mod testkit {
    use super::*;

    /// Records every request and answers by a rule.
        #[derive(Default)]
        pub struct Fake {
            pub calls: Mutex<Vec<(String, Vec<(String, String)>, Vec<u8>)>>,
            /// (part of the address, status, body)
            pub rules: Mutex<Vec<(String, u16, String)>>,
        }

        impl Transport for Fake {
            fn post(&self, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<(u16, String), String> {
                self.calls.lock().unwrap().push((url.to_string(), headers.to_vec(), body.to_vec()));
                for (p, s, b) in self.rules.lock().unwrap().iter() {
                    if url.contains(p.as_str()) {
                        return Ok((*s, b.clone()));
                    }
                }
                Ok((201, String::new()))
            }
        }

    /// A browser's subscription keys: its private key and its auth secret.
    pub fn browser() -> (SecretKey, [u8; 16]) {
        (new_secret(), random_bytes::<16>())
    }

    /// What the browser sends to `push_subscribe`.
    pub fn web_sub(ep: &str, k: &(SecretKey, [u8; 16])) -> Value {
        json!({"kind": "webpush", "label": "Firefox on Linux", "subscription": {"endpoint": ep, "keys": {"p256dh": B64.encode(public_bytes(&k.0)), "auth": B64.encode(k.1)}}})
    }

    /// The receiver's side: what a browser that holds these keys reads from a push body.
    pub fn decrypt_for(k: &(SecretKey, [u8; 16]), body: &[u8]) -> Value {
        let ua_pub = public_bytes(&k.0);
        let (salt, idlen) = (&body[..16], body[20] as usize);
        let as_pub = &body[21..21 + idlen];
        let shared = p256::ecdh::diffie_hellman(k.0.to_nonzero_scalar(), PublicKey::from_sec1_bytes(as_pub).unwrap().as_affine());
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(&ua_pub);
        info.extend_from_slice(as_pub);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(&k.1), shared.raw_secret_bytes().as_slice()).expand(&info, &mut ikm).unwrap();
        let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
        prk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
        let plain = Aes128Gcm::new_from_slice(&cek).unwrap().decrypt(Nonce::from_slice(&nonce), &body[21 + idlen..]).unwrap();
        serde_json::from_slice(&plain[..plain.len() - 1]).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemDb;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;
    use super::testkit::Fake;

    fn b(s: &str) -> Vec<u8> {
        unb64(s).unwrap()
    }

    /// RFC 8291 section 5 and appendix A: the very example of the standard, byte for byte.
    #[test]
    fn rfc8291_example_is_reproduced() {
        let as_secret = SecretKey::from_slice(&b("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        let ua_public = b("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        assert_eq!(public_bytes(&as_secret), b("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"));
        let salt: [u8; 16] = b("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let out = encrypt(&ua_public, &b("BTBZMqHH6r4Tts7J_aSIgg"), b"When I grow up, I want to be a watermelon", &as_secret, &salt).unwrap();
        let want = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";
        assert_eq!(B64.encode(&out), want);
        // 86 (header) + 41 (text) + 1 (delimiter) + 16 (tag); the example's printed "Content-Length: 145" does not match its own body bytes
        assert_eq!(out.len(), 144);
    }

    /// The receiving side, written from the RFC's recipe with the receiver's private key: what we send can be read.
    #[test]
    fn encrypted_payload_round_trips_for_the_receiver() {
        let ua_secret = SecretKey::from_slice(&b("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94")).unwrap();
        let (ua_pub, auth) = (public_bytes(&ua_secret), b("BTBZMqHH6r4Tts7J_aSIgg"));
        let msg = br#"{"v":1,"type":"answer","chat":"c1"}"#;
        let body = encrypt(&ua_pub, &auth, msg, &new_secret(), &random_bytes::<16>()).unwrap();
        let (salt, rs, idlen) = (&body[..16], u32::from_be_bytes(body[16..20].try_into().unwrap()), body[20] as usize);
        assert_eq!((rs, idlen), (4096, 65));
        let as_pub = &body[21..21 + idlen];
        let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), PublicKey::from_sec1_bytes(as_pub).unwrap().as_affine());
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(&ua_pub);
        info.extend_from_slice(as_pub);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes().as_slice()).expand(&info, &mut ikm).unwrap();
        let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
        prk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
        let plain = Aes128Gcm::new_from_slice(&cek).unwrap().decrypt(Nonce::from_slice(&nonce), &body[21 + idlen..]).unwrap();
        assert_eq!(&plain[..plain.len() - 1], msg);
        assert_eq!(plain[plain.len() - 1], 2);
        // a wrong auth secret cannot read it
        assert!(encrypt(&ua_pub, &[0u8; 15], msg, &new_secret(), &[0u8; 16]).is_err());
        assert!(encrypt(&ua_pub[..64], &auth, msg, &new_secret(), &[0u8; 16]).is_err());
        assert!(encrypt(&ua_pub, &auth, &vec![b'x'; MAX_PLAIN + 1], &new_secret(), &[0u8; 16]).is_err());
    }

    /// RFC 8292 section 2: the example token. The first two parts are reproduced exactly; the signature checks against the key of
    /// the example (ECDSA is randomised in general, ours is deterministic (RFC 6979), so the signature itself is verified, not compared).
    #[test]
    fn rfc8292_example_token() {
        let rfc = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiJ9.eyJhdWQiOiJodHRwczovL3B1c2guZXhhbXBsZS5uZXQiLCJleHAiOjE0NTM1MjM3NjgsInN1YiI6Im1haWx0bzpwdXNoQGV4YW1wbGUuY29tIn0.i3CYb7t4xfxCDquptFOepC9GAu_HLGkMlMuCGSK2rpiUfnK9ojFwDXb1JrErtmysazNjjvW2L9OkSSHzvoD1oA";
        let key = SigningKey::from(&new_secret());
        let mine = vapid_jwt(&key, "https://push.example.net", "mailto:push@example.com", 1453523768);
        assert_eq!(mine.rsplit_once('.').unwrap().0, rfc.rsplit_once('.').unwrap().0, "header and claims are exactly the RFC's");
        // the RFC's signature verifies with the RFC's public key (the JWK of its figure 2)
        let (input, sig) = rfc.rsplit_once('.').unwrap();
        let k = VerifyingKey::from_sec1_bytes(&[vec![4u8], b("DUfHPKLVFQzVvnCPGyfucbECzPDa7rWbXriLcysAjEc"), b("F6YK5h4SDYic-dRuU_RCPCfA5aq9ojSwk5Y2EmClBPs")].concat().as_slice()).unwrap();
        let sig = Signature::from_slice(&b(sig)).unwrap();
        assert!(k.verify(input.as_bytes(), &sig).is_ok(), "the RFC's own signature verifies");
        // and ours verifies with our key, and not with another
        let (min, msig) = mine.rsplit_once('.').unwrap();
        assert!(key.verifying_key().verify(min.as_bytes(), &Signature::from_slice(&b(msig)).unwrap()).is_ok());
        assert!(k.verify(min.as_bytes(), &Signature::from_slice(&b(msig)).unwrap()).is_err());
        let h = vapid_header(&new_secret(), "https://fcm.googleapis.com", "mailto:a@b.example", 1);
        assert!(h.starts_with("vapid t=") && h.contains(", k=B"));
    }

    #[test]
    fn the_allow_list_refuses_everything_but_the_push_services() {
        let a = Allow::default();
        for ok in [
            "https://fcm.googleapis.com/fcm/send/abc:def",
            "https://updates.push.services.mozilla.com/wpush/v2/gAAA",
            "https://web.push.apple.com/QGxx",
            "https://x.push.apple.com/abc",
            "https://wns2-par02p.notify.windows.com/w/?token=abc",
            "https://FCM.googleapis.com:443/x",
        ] {
            assert!(a.check(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://fcm.googleapis.com/fcm/send/abc",
            "https://fcm.googleapis.com.evil.example/x",
            "https://evil.example/fcm.googleapis.com",
            "https://fcm.googleapis.com@evil.example/x",
            "https://evil.example@fcm.googleapis.com/x",
            "https://fcm.googleapis.com:8443/x",
            "https://127.0.0.1/x",
            "https://localhost/x",
            "https://[::1]/x",
            "https://169.254.169.254/latest/meta-data",
            "https://push.apple.com/x",
            "https://notify.windows.com/x",
            "https://evilnotify.windows.com/x",
            "https://x.notify.windows.com.evil.example/x",
            "https://fcm.googleapis.com",
            "https://fcm.googleapis.com\\@evil.example/x",
            "https://fcm.googleapis.com/x y",
            "https://x..push.apple.com/x",
            "ftp://fcm.googleapis.com/x",
            "file:///etc/passwd",
            "//fcm.googleapis.com/x",
            "",
        ] {
            assert!(a.check(bad).is_err(), "{bad}");
        }
        assert!(a.check(&format!("https://fcm.googleapis.com/{}", "a".repeat(MAX_ENDPOINT))).is_err(), "too long");
        assert_eq!(a.check("https://x.push.apple.com/abc").unwrap(), "https://x.push.apple.com");
    }

    fn browser() -> (SecretKey, [u8; 16]) {
        testkit::browser()
    }

    fn web_sub(ep: &str, k: &(SecretKey, [u8; 16])) -> Value {
        testkit::web_sub(ep, k)
    }

    fn rig() -> (Arc<Hub>, Arc<Fake>, Arc<MemDb>) {
        let (fake, db) = (Arc::new(Fake::default()), Arc::new(MemDb::default()));
        let dbd: Arc<dyn Db> = db.clone();
        let hub = Hub::new(Some(dbd), "test-secret", "mailto:o@zo.test".into()).with_transport(fake.clone()).with_fcm("proj-1", "ya29.token");
        (Arc::new(hub), fake, db)
    }

    #[test]
    fn subscriptions_are_validated_limited_and_listed_without_their_addresses() {
        let (hub, _, db) = rig();
        let t = "a".repeat(32);
        let k = browser();
        assert!(hub.subscribe(&t, &web_sub("https://evil.example/x", &k)).is_err(), "SSRF address");
        assert!(hub.subscribe(&t, &web_sub("http://fcm.googleapis.com/x", &k)).is_err());
        let mut bad = web_sub("https://fcm.googleapis.com/fcm/send/1", &k);
        bad["subscription"]["keys"]["auth"] = json!(B64.encode([1u8; 8]));
        assert_eq!(hub.subscribe(&t, &bad).unwrap_err().0, 400, "short auth secret");
        bad["subscription"]["keys"] = json!({"p256dh": B64.encode([4u8; 65]), "auth": B64.encode([1u8; 16])});
        assert_eq!(hub.subscribe(&t, &bad).unwrap_err().0, 400, "not a point on the curve");
        assert!(hub.subscribe(&t, &json!({"kind": "carrier-pigeon"})).is_err());
        assert!(hub.subscribe(&t, &json!({"kind": "fcm", "token": "short"})).is_err());
        assert!(hub.subscribe(&t, &json!({"kind": "fcm", "token": format!("{}!", "a".repeat(30))})).is_err());
        assert_eq!(db.count_under(&format!("users/{t}")), 0, "nothing was stored for any of those");
        // five are fine, the sixth is refused, registering the same one again replaces it
        let mut ids = Vec::new();
        for n in 0..MAX_SUBS {
            ids.push(hub.subscribe(&t, &web_sub(&format!("https://fcm.googleapis.com/fcm/send/{n}"), &k)).unwrap());
        }
        assert_eq!(hub.subscribe(&t, &web_sub("https://fcm.googleapis.com/fcm/send/99", &k)).unwrap_err().0, 409);
        assert_eq!(hub.subscribe(&t, &web_sub("https://fcm.googleapis.com/fcm/send/2", &k)).unwrap(), ids[2], "same endpoint, same entry");
        assert_eq!(hub.subscribe(&t, &json!({"kind": "fcm", "token": "a".repeat(40)})).unwrap_err().0, 409);
        assert_eq!(hub.list(&t).unwrap().len(), MAX_SUBS);
        let listed = hub.list(&t).unwrap().to_vec();
        assert!(listed.iter().all(|s| s.get("endpoint").is_none() && s.get("p256dh").is_none() && s["label"] == "Firefox on Linux"));
        // another person has their own list
        let u = "b".repeat(32);
        assert!(hub.list(&u).unwrap().is_empty() && !hub.has_any(&u));
        assert!(!hub.unsubscribe(&u, &ids[0]).unwrap(), "cannot remove somebody else's");
        assert!(hub.unsubscribe(&t, &ids[0]).unwrap());
        assert_eq!(hub.list(&t).unwrap().len(), MAX_SUBS - 1);
        // it survives a restart: a new hub reads the database
        let again = Hub::new(Some(db.clone()), "test-secret", "mailto:o@zo.test".into());
        assert_eq!(again.list(&t).unwrap().len(), MAX_SUBS - 1);
    }

    #[test]
    fn a_web_push_is_encrypted_signed_and_sent_to_the_browsers_service_only() {
        let (hub, fake, _) = rig();
        let (t, k) = ("c".repeat(32), browser());
        let ep = "https://updates.push.services.mozilla.com/wpush/v2/abc";
        hub.subscribe(&t, &web_sub(ep, &k)).unwrap();
        assert_eq!(hub.deliver_answer(&t, &Note { chat: "c1700", title: "Secret plans for dinner" }), 1);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (url, headers, body) = &calls[0];
        assert_eq!(url, ep);
        let h = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(h("Content-Encoding"), "aes128gcm");
        assert!(h("Authorization").starts_with("vapid t="));
        // the VAPID token names the service's origin and our public key
        let auth = h("Authorization");
        let jwt = auth.strip_prefix("vapid t=").unwrap().split(',').next().unwrap();
        let claims: Value = serde_json::from_slice(&b(jwt.split('.').nth(1).unwrap())).unwrap();
        assert_eq!(claims["aud"], "https://updates.push.services.mozilla.com");
        assert_eq!(claims["sub"], "mailto:o@zo.test");
        assert!(claims["exp"].as_u64().unwrap() <= now_secs() + 24 * 3600);
        assert!(auth.ends_with(&format!("k={}", hub.public_key().unwrap())));
        // the browser (we hold its private key here) reads: generic text, chat id, and NO title (the person did not allow it)
        let plain = decrypt_for(&k, body);
        assert_eq!(plain, json!({"v": 1, "type": "answer", "chat": "c1700"}));
        assert!(!String::from_utf8_lossy(body).contains("Secret"));
    }

    fn decrypt_for(k: &(SecretKey, [u8; 16]), body: &[u8]) -> Value {
        testkit::decrypt_for(k, body)
    }

    #[test]
    fn the_title_is_sent_only_when_allowed_and_cut_to_60_characters() {
        let (hub, fake, _) = rig();
        let (t, k) = ("d".repeat(32), browser());
        let mut s = web_sub("https://fcm.googleapis.com/fcm/send/z", &k);
        s["show_title"] = json!(true);
        hub.subscribe(&t, &s).unwrap();
        let long = format!("{}\n\u{7}end", "ä".repeat(100));
        hub.deliver_answer(&t, &Note { chat: "c1", title: &long });
        let body = fake.calls.lock().unwrap()[0].2.clone();
        let p = decrypt_for(&k, &body);
        let title = p["title"].as_str().unwrap();
        assert_eq!(title.chars().count(), TITLE_CHARS);
        assert!(title.ends_with('…') && !title.chars().any(|c| c.is_control()));
        // a chat id that is not a plain id is dropped, never forwarded
        hub.deliver_answer(&t, &Note { chat: "../../etc", title: "" });
        let p = decrypt_for(&k, &fake.calls.lock().unwrap()[1].2.clone());
        assert_eq!(p["chat"], "");
        assert!(p.get("title").is_none());
    }

    #[test]
    fn expired_subscriptions_are_removed_and_the_others_still_get_the_push() {
        let (hub, fake, db) = rig();
        let (t, k) = ("e".repeat(32), browser());
        let gone = hub.subscribe(&t, &web_sub("https://fcm.googleapis.com/fcm/send/gone", &k)).unwrap();
        let moved = hub.subscribe(&t, &web_sub("https://fcm.googleapis.com/fcm/send/410", &k)).unwrap();
        let keep = hub.subscribe(&t, &web_sub("https://updates.push.services.mozilla.com/wpush/v2/keep", &k)).unwrap();
        let broken = hub.subscribe(&t, &web_sub("https://web.push.apple.com/broken", &k)).unwrap();
        *fake.rules.lock().unwrap() = vec![("send/gone".into(), 404, "".into()), ("send/410".into(), 410, "".into()), ("apple".into(), 500, "oops".into())];
        assert_eq!(hub.deliver_answer(&t, &Note { chat: "c1", title: "" }), 1);
        let ids: Vec<String> = hub.list(&t).unwrap().iter().map(|s| s["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids, vec![keep.clone(), broken.clone()], "404 and 410 removed, a 500 is only a failure");
        assert!(!ids.contains(&gone) && !ids.contains(&moved));
        // the removal is in the database too
        let again = Hub::new(Some(db.clone()), "test-secret", "mailto:o@zo.test".into());
        assert_eq!(again.list(&t).unwrap().len(), 2);
    }

    #[test]
    fn fcm_messages_use_the_service_account_and_unregistered_tokens_go() {
        let (hub, fake, _) = rig();
        let t = "f".repeat(32);
        let id = hub.subscribe(&t, &json!({"kind": "fcm", "token": "tok_A-1:2".repeat(8), "label": "Pixel"})).unwrap();
        assert_eq!(hub.deliver_answer(&t, &Note { chat: "c9", title: "Hidden" }), 1);
        {
            let calls = fake.calls.lock().unwrap();
            let (url, headers, body) = &calls[0];
            assert_eq!(url, "https://fcm.googleapis.com/v1/projects/proj-1/messages:send");
            assert!(headers.contains(&("Authorization".into(), "Bearer ya29.token".into())));
            let m: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(m["message"]["notification"]["body"], "Your answer is ready");
            assert_eq!(m["message"]["data"]["chat"], "c9");
            assert!(!String::from_utf8_lossy(body).contains("Hidden"));
        }
        *fake.rules.lock().unwrap() = vec![("messages:send".into(), 404, r#"{"error":{"status":"NOT_FOUND","details":[{"errorCode":"UNREGISTERED"}]}}"#.into())];
        assert_eq!(hub.deliver_answer(&t, &Note { chat: "c9", title: "" }), 0);
        assert!(hub.list(&t).unwrap().is_empty(), "unregistered token removed ({id})");
        // a server without a service account refuses fcm registrations
        let plain = Hub::new(Some(Arc::new(MemDb::default())), "s", "mailto:o@zo.test".into());
        assert!(plain.subscribe(&t, &json!({"kind": "fcm", "token": "a".repeat(40)})).is_err());
        assert_eq!(plain.account_call("push_config", &json!({}), &t).1["fcm"], false);
    }

    #[test]
    fn at_most_30_pushes_an_hour_per_account() {
        let (hub, fake, _) = rig();
        let (t, u, k) = ("1".repeat(32), "2".repeat(32), browser());
        hub.subscribe(&t, &web_sub("https://fcm.googleapis.com/fcm/send/1", &k)).unwrap();
        hub.subscribe(&u, &web_sub("https://fcm.googleapis.com/fcm/send/2", &k)).unwrap();
        for _ in 0..PUSHES_PER_HOUR + 5 {
            hub.deliver_answer(&t, &Note { chat: "c", title: "" });
        }
        assert_eq!(fake.calls.lock().unwrap().len(), PUSHES_PER_HOUR);
        assert_eq!(hub.deliver_answer(&u, &Note { chat: "c", title: "" }), 1, "another account is not affected");
    }

    #[test]
    fn the_vapid_key_is_made_once_kept_encrypted_and_survives_a_restart() {
        let db = Arc::new(MemDb::default());
        let dbd: Arc<dyn Db> = db.clone();
        let a = Hub::new(Some(dbd.clone()), "secret-one", "mailto:o@zo.test".into());
        let key = a.public_key().unwrap();
        assert_eq!(unb64(&key).unwrap().len(), 65);
        let stored = db.get_doc(SERVER_COLLECTION, VAPID_DOC).unwrap().unwrap();
        let private = B64.encode(a.vapid().unwrap().secret.to_bytes());
        assert!(!stored.contains(&private), "the private key is not stored in the clear");
        // a restart: the same key
        assert_eq!(Hub::new(Some(dbd.clone()), "secret-one", "x".into()).public_key().unwrap(), key);
        // another secret cannot open it and must not replace it
        let other = Hub::new(Some(dbd.clone()), "secret-two", "x".into());
        assert!(other.public_key().unwrap_err().contains("ZO_SERVER_SECRET"));
        assert_eq!(db.get_doc(SERVER_COLLECTION, VAPID_DOC).unwrap().unwrap(), stored);
        // without any secret nothing is stored
        let none = Hub::new(Some(Arc::new(MemDb::default())), "", "x".into());
        assert!(none.public_key().is_err());
        assert_eq!(none.account_call("push_subscribe", &json!({"kind": "webpush"}), &"a".repeat(32)).0, 503);
        // test mode (no database): a key for this run
        assert!(Hub::default().public_key().is_ok());
    }

    #[test]
    fn account_calls_follow_the_contract() {
        let (hub, _, _) = rig();
        let (t, k) = ("9".repeat(32), browser());
        let c = hub.account_call("push_config", &json!({}), &t);
        assert_eq!((c.0, c.1["fcm"].clone()), (200, json!(true)));
        assert_eq!(c.1["vapid_public_key"].as_str().unwrap(), hub.public_key().unwrap());
        let s = hub.account_call("push_subscribe", &web_sub("https://fcm.googleapis.com/fcm/send/q", &k), &t);
        assert_eq!(s.0, 200);
        let id = s.1["id"].as_str().unwrap().to_string();
        let l = hub.account_call("push_list", &json!({}), &t).1;
        assert_eq!(l["subscriptions"][0]["id"], id.as_str());
        assert_eq!(l["subscriptions"][0]["kind"], "webpush");
        assert_eq!(hub.account_call("push_unsubscribe", &json!({"id": "nope"}), &t).0, 404);
        assert_eq!(hub.account_call("push_unsubscribe", &json!({"id": id}), &t).0, 200);
        assert!(hub.account_call("push_list", &json!({}), &t).1["subscriptions"].as_array().unwrap().is_empty());
    }
}
