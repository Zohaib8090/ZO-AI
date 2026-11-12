//! Who is asking. A person signs in with Google through Firebase; every request carries the Firebase ID token. The token is checked
//! against Google's public keys, must be for this project, must come from Google sign-in, and must have a verified e-mail address.
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

static KEYS: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

/// A signed-in person, as far as the token tells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Firebase's user id (never used in a path: see `tenant_id`).
    pub uid: String,
    /// Lower case, and verified by Google.
    pub email: String,
}

pub trait Verifier: Send + Sync {
    fn verify(&self, token: &str) -> Result<Identity, String>;
}

pub struct Firebase {
    pub project: String,
}

fn google_keys() -> Result<Value, String> {
    let mut g = KEYS.lock().unwrap();
    if let Some((at, v)) = g.as_ref() {
        if at.elapsed() < Duration::from_secs(3600) {
            return Ok(v.clone());
        }
    }
    let v: Value = ureq::get(KEYS_URL).timeout(Duration::from_secs(15)).call().map_err(|e| format!("cannot fetch Google's keys: {e}"))?.into_json().map_err(|e| e.to_string())?;
    *g = Some((Instant::now(), v.clone()));
    Ok(v)
}

impl Verifier for Firebase {
    fn verify(&self, token: &str) -> Result<Identity, String> {
        let kid = decode_header(token).map_err(|_| "The sign-in token is not valid.")?.kid.ok_or("The sign-in token has no key id.")?;
        let keys = google_keys()?;
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
    if uid.is_empty() || email.is_empty() || !verified || provider != "google.com" {
        return Err("Sign in with a Google account whose e-mail address is verified.".into());
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
    fn any_verified_google_account_is_accepted_and_nothing_else() {
        let ok = json!({"sub": "u1", "email": " Me@Example.COM ", "email_verified": true, "firebase": {"sign_in_provider": "google.com"}});
        assert_eq!(identity_from_claims(&ok).unwrap(), Identity { uid: "u1".into(), email: "me@example.com".into() });
        for bad in [
            json!({"sub": "u1", "email": "a@b.c", "email_verified": false, "firebase": {"sign_in_provider": "google.com"}}),
            json!({"sub": "u1", "email": "a@b.c", "firebase": {"sign_in_provider": "google.com"}}),
            json!({"sub": "u1", "email": "a@b.c", "email_verified": true, "firebase": {"sign_in_provider": "password"}}),
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
