//! Who is asking. A person signs in with Google or with e-mail and password through Firebase; every request carries the Firebase ID
//! token. The token is checked against Google's public keys, must be for this project, must come from Google or e-mail/password
//! sign-in, and must have a verified e-mail address (an e-mail account that never confirmed its address is refused like any other).
//! After that, who the person is (admin, restricted, blocked) is decided by `Policy`, from the server's settings.
//!
//! `Verifier` is a trait so the tests can stand in identities without Google.

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const KEYS_URL: &str = "https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com";

/// A signed-in person, as far as the token tells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Firebase's user id (never used in a path: see `tenant_id`).
    pub uid: String,
    /// Lower case, and verified (by Google, or by the link Firebase sent to an e-mail/password account).
    pub email: String,
}

pub trait Verifier: Send + Sync {
    fn verify(&self, token: &str) -> Result<Identity, String>;
}

pub struct Firebase {
    pub project: String,
}

/// Google's signing keys, remembered as long as Google says (Cache-Control), with these rules: one fetch at a time and nobody else
/// waits on the network (they wait for that one fetch, then read the cache); a failed fetch is not tried again for `RETRY_AFTER`;
/// keys that are too old are still used when Google cannot be reached (a short outage does not sign everybody out); a token with a key
/// id that is not in the cache makes one new fetch (a rotation), but not more than one a minute.
struct Cache {
    keys: Option<Value>,
    expires: Option<Instant>,
    fetched: Option<Instant>,
    failed: Option<Instant>,
}

static KEYS: Mutex<Cache> = Mutex::new(Cache { keys: None, expires: None, fetched: None, failed: None });
static FETCHING: Mutex<()> = Mutex::new(());
const RETRY_AFTER: Duration = Duration::from_secs(10);
const ROTATION_GAP: Duration = Duration::from_secs(60);
const STALE_OK: Duration = Duration::from_secs(12 * 3600);

fn has_kid(keys: &Value, kid: &str) -> bool {
    keys.get("keys").and_then(Value::as_array).is_some_and(|a| a.iter().any(|k| k.get("kid").and_then(Value::as_str) == Some(kid)))
}

fn fetch_google() -> Result<(Value, Duration), String> {
    let r = ureq::get(KEYS_URL).timeout(Duration::from_secs(15)).call().map_err(|e| format!("cannot fetch Google's keys: {e}"))?;
    let age = r.header("cache-control").and_then(|c| c.split(',').find_map(|p| p.trim().strip_prefix("max-age=").and_then(|n| n.parse::<u64>().ok()))).unwrap_or(3600).clamp(300, 86_400);
    let v: Value = r.into_json().map_err(|e| e.to_string())?;
    Ok((v, Duration::from_secs(age)))
}

fn google_keys(kid: &str) -> Result<Value, String> {
    keys_from(&KEYS, &FETCHING, Instant::now(), kid, &fetch_google)
}

fn keys_from(cache: &Mutex<Cache>, gate: &Mutex<()>, now: Instant, kid: &str, fetch: &dyn Fn() -> Result<(Value, Duration), String>) -> Result<Value, String> {
    // the cached keys, when they are good for this token
    let usable = |c: &Cache| -> Option<Value> {
        let v = c.keys.as_ref()?;
        let fresh = c.expires.is_some_and(|e| now < e);
        let rotated = !has_kid(v, kid) && c.fetched.is_none_or(|f| now.saturating_duration_since(f) >= ROTATION_GAP);
        (fresh && !rotated).then(|| v.clone())
    };
    if let Some(v) = usable(&cache.lock().unwrap_or_else(|e| e.into_inner())) {
        return Ok(v);
    }
    let _one = gate.lock().unwrap_or_else(|e| e.into_inner());
    // somebody else may have fetched while we waited
    {
        let c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = usable(&c) {
            return Ok(v);
        }
        if c.failed.is_some_and(|f| now.saturating_duration_since(f) < RETRY_AFTER) {
            return stale(&c, now);
        }
    }
    match fetch() {
        Ok((v, ttl)) => {
            let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
            *c = Cache { keys: Some(v.clone()), expires: Some(now + ttl), fetched: Some(now), failed: None };
            Ok(v)
        }
        Err(e) => {
            let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
            c.failed = Some(now);
            stale(&c, now).map_err(|_| e)
        }
    }
}

/// Keys that have run out, used because Google cannot be reached.
fn stale(c: &Cache, now: Instant) -> Result<Value, String> {
    match (&c.keys, c.fetched) {
        (Some(v), Some(f)) if now.saturating_duration_since(f) < STALE_OK => Ok(v.clone()),
        _ => Err("Google's sign-in keys cannot be fetched right now.".into()),
    }
}

impl Verifier for Firebase {
    fn verify(&self, token: &str) -> Result<Identity, String> {
        let kid = decode_header(token).map_err(|_| "The sign-in token is not valid.")?.kid.ok_or("The sign-in token has no key id.")?;
        let keys = google_keys(&kid)?;
        let jwk = keys.get("keys").and_then(Value::as_array).and_then(|a| a.iter().find(|k| k.get("kid").and_then(Value::as_str) == Some(&kid))).ok_or("The sign-in token was signed with an unknown key.")?;
        let (n, e) = (jwk.get("n").and_then(Value::as_str).unwrap_or(""), jwk.get("e").and_then(Value::as_str).unwrap_or(""));
        let key = DecodingKey::from_rsa_components(n, e).map_err(|e| e.to_string())?;
        let mut v = Validation::new(Algorithm::RS256);
        v.set_audience(&[&self.project]);
        v.set_issuer(&[format!("https://securetoken.google.com/{}", self.project)]);
        let claims = decode::<Value>(token, &key, &v).map_err(|e| format!("The sign-in is not accepted: {e}"))?.claims;
        identity_from_claims(&claims)
    }
}

/// The checks on a token's contents (after its signature, audience and issuer were verified).
pub fn identity_from_claims(claims: &Value) -> Result<Identity, String> {
    let uid = claims.get("sub").and_then(Value::as_str).unwrap_or("");
    let email = claims.get("email").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
    let verified = claims.get("email_verified").and_then(Value::as_bool).unwrap_or(false);
    let provider = claims.pointer("/firebase/sign_in_provider").and_then(Value::as_str).unwrap_or("");
    if uid.is_empty() || email.is_empty() || !verified || !matches!(provider, "google.com" | "password") {
        return Err("Sign in with a Google account or an e-mail account whose address is verified.".into());
    }
    Ok(Identity { uid: uid.to_string(), email })
}

/// The id of a person's corner of the server: the first 32 hex characters of sha256("zo-tenant:" + Firebase uid). Neither the uid
/// nor the e-mail address is ever used in a file path or a Firestore path.
pub fn tenant_id(uid: &str) -> String {
    let d = Sha256::digest(format!("zo-tenant:{uid}").as_bytes());
    d[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// A short fingerprint of an e-mail address: lets a device token notice that its owner was blocked or is an admin now, without the
/// address itself being stored anywhere.
pub fn email_hash(email: &str) -> String {
    let d = Sha256::digest(format!("zo-email:{}", email.trim().to_lowercase()).as_bytes());
    d[..12].iter().map(|b| format!("{b:02x}")).collect()
}

/// What the server's settings say about people.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    admins: HashSet<String>,
    blocked: HashSet<String>,
    pub max_users: usize,
}

fn list(v: Option<String>) -> HashSet<String> {
    v.map(|s| s.split(',').map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()).collect()).unwrap_or_default()
}

impl Policy {
    pub fn new(admins: Option<String>, blocked: Option<String>, max_users: Option<String>) -> Policy {
        let max_users = max_users.and_then(|m| m.trim().parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or(200);
        Policy { admins: list(admins), blocked: list(blocked), max_users }
    }

    pub fn is_admin(&self, email: &str) -> bool {
        self.admins.contains(&email.trim().to_lowercase())
    }

    pub fn is_blocked(&self, email: &str) -> bool {
        self.blocked.contains(&email.trim().to_lowercase())
    }

    /// The same two questions for a device token, which knows only the fingerprint of its owner's address.
    pub fn is_admin_hash(&self, h: &str) -> bool {
        self.admins.iter().any(|e| email_hash(e) == h)
    }

    pub fn is_blocked_hash(&self, h: &str) -> bool {
        self.blocked.iter().any(|e| email_hash(e) == h)
    }

    pub fn admin_count(&self) -> usize {
        self.admins.len()
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn cache() -> Mutex<Cache> {
        Mutex::new(Cache { keys: None, expires: None, fetched: None, failed: None })
    }

    fn keys(kids: &[&str]) -> Value {
        serde_json::json!({"keys": kids.iter().map(|k| serde_json::json!({"kid": k})).collect::<Vec<_>>()})
    }

    #[test]
    fn google_keys_are_cached_survive_an_outage_follow_a_rotation_and_never_make_everybody_wait() {
        let (c, gate, n) = (cache(), Mutex::new(()), AtomicUsize::new(0));
        let t0 = Instant::now();
        let ok = |kids: &'static [&'static str]| move || -> Result<(Value, Duration), String> { Ok((keys(kids), Duration::from_secs(3600))) };
        let count = |f: &dyn Fn() -> Result<(Value, Duration), String>| { n.fetch_add(1, Ordering::SeqCst); f() };
        let a = ok(&["k1"]);
        assert!(has_kid(&keys_from(&c, &gate, t0, "k1", &|| count(&a)).unwrap(), "k1"));
        keys_from(&c, &gate, t0 + Duration::from_secs(100), "k1", &|| count(&a)).unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 1, "cached");
        // an unknown key id makes one new fetch (a rotation), then at most one a minute
        let b = ok(&["k1", "k2"]);
        let t1 = t0 + Duration::from_secs(200);
        assert!(has_kid(&keys_from(&c, &gate, t1, "k2", &|| count(&b)).unwrap(), "k2"));
        assert_eq!(n.load(Ordering::SeqCst), 2);
        keys_from(&c, &gate, t1 + Duration::from_secs(5), "bogus", &|| count(&b)).unwrap();
        keys_from(&c, &gate, t1 + Duration::from_secs(6), "bogus2", &|| count(&b)).unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 2, "a made-up key id cannot make the server fetch again and again");
        // Google is down after the keys expired: the old keys still serve, and the failure is not retried by every request
        let down = || -> Result<(Value, Duration), String> { Err("down".into()) };
        let late = t0 + Duration::from_secs(4000);
        assert!(has_kid(&keys_from(&c, &gate, late, "k1", &|| count(&down)).unwrap(), "k1"));
        assert_eq!(n.load(Ordering::SeqCst), 3);
        keys_from(&c, &gate, late + Duration::from_secs(2), "k1", &|| count(&down)).unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 3, "no retry within the pause");
        // with nothing cached at all, an outage is an error (and also not hammered)
        let empty = cache();
        assert!(keys_from(&empty, &gate, t0, "k1", &down).is_err());
        // the cache lock is not held during a fetch: a slow fetch does not block a reader of fresh keys
        assert!(c.try_lock().is_ok());
    }
}

#[cfg(test)]
pub struct Fake(pub std::sync::Mutex<std::collections::HashMap<String, Identity>>);

#[cfg(test)]
impl Fake {
    pub fn new(people: &[(&str, &str, &str)]) -> Fake {
        Fake(std::sync::Mutex::new(people.iter().map(|(t, u, e)| (t.to_string(), Identity { uid: u.to_string(), email: e.to_string() })).collect()))
    }
}

#[cfg(test)]
impl Verifier for Fake {
    fn verify(&self, token: &str) -> Result<Identity, String> {
        self.0.lock().unwrap().get(token).cloned().ok_or_else(|| "The sign-in token is not valid.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_tenant_id_is_32_hex_chars_stable_and_hides_the_uid() {
        let a = tenant_id("uid-123");
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(a, tenant_id("uid-123"));
        assert_ne!(a, tenant_id("uid-124"));
        assert!(!a.contains("uid"));
        // the documented derivation: sha256("zo-tenant:" + uid), first 16 bytes
        let direct: String = Sha256::digest(b"zo-tenant:uid-123")[..16].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(a, direct);
        assert!(zo_core::tenant::valid_id(&a));
    }

    #[test]
    fn any_verified_google_or_email_account_is_accepted_and_nothing_else() {
        let ok = json!({"sub": "u1", "email": " Me@Example.COM ", "email_verified": true, "firebase": {"sign_in_provider": "google.com"}});
        assert_eq!(identity_from_claims(&ok).unwrap(), Identity { uid: "u1".into(), email: "me@example.com".into() });
        // an e-mail and password account counts when (and only when) its address was verified
        let pw = json!({"sub": "u2", "email": "Pw@Example.com", "email_verified": true, "firebase": {"sign_in_provider": "password"}});
        assert_eq!(identity_from_claims(&pw).unwrap(), Identity { uid: "u2".into(), email: "pw@example.com".into() });
        for bad in [
            json!({"sub": "u1", "email": "a@b.c", "email_verified": false, "firebase": {"sign_in_provider": "google.com"}}),
            json!({"sub": "u1", "email": "a@b.c", "firebase": {"sign_in_provider": "google.com"}}),
            json!({"sub": "u1", "email": "a@b.c", "email_verified": false, "firebase": {"sign_in_provider": "password"}}),
            json!({"sub": "u1", "email": "a@b.c", "firebase": {"sign_in_provider": "password"}}),
            json!({"sub": "u1", "email": "a@b.c", "email_verified": true, "firebase": {"sign_in_provider": "custom"}}),
            json!({"sub": "u1", "email": "a@b.c", "email_verified": true, "firebase": {"sign_in_provider": "phone"}}),
            json!({"sub": "u1", "email": "a@b.c", "email_verified": true, "firebase": {"sign_in_provider": "anonymous"}}),
            json!({"sub": "u1", "email": "a@b.c", "email_verified": true}),
            json!({"sub": "", "email": "a@b.c", "email_verified": true, "firebase": {"sign_in_provider": "google.com"}}),
            json!({"sub": "u1", "email": "", "email_verified": true, "firebase": {"sign_in_provider": "google.com"}}),
        ] {
            assert!(identity_from_claims(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn admins_and_blocked_lists_work_by_address_and_by_fingerprint() {
        let p = Policy::new(Some(" Owner@X.com, ,second@x.com".into()), Some("bad@x.com".into()), Some("3".into()));
        assert!(p.is_admin("owner@x.com") && p.is_admin("OWNER@x.com") && p.is_admin("second@x.com") && !p.is_admin("other@x.com"));
        assert!(p.is_blocked("Bad@x.com") && !p.is_blocked("owner@x.com"));
        assert!(p.is_admin_hash(&email_hash("owner@x.com")) && !p.is_admin_hash(&email_hash("other@x.com")));
        assert!(p.is_blocked_hash(&email_hash("BAD@x.com")));
        assert_eq!(p.max_users, 3);
        assert_eq!(Policy::new(None, None, None).max_users, 200);
        assert_eq!(Policy::new(None, None, Some("0".into())).max_users, 200);
        assert_eq!(Policy::new(None, None, Some("abc".into())).max_users, 200);
        assert!(!Policy::new(None, None, None).is_admin(""), "an empty list admits nobody");
    }
}
