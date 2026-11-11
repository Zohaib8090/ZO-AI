//! Who is asking: every request carries a Firebase ID token (the web app gets it from Firebase sign-in). The token is checked
//! against Google's public keys, must be for this project, and must belong to the one allowed e-mail address.

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const KEYS_URL: &str = "https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com";

static KEYS: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

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

/// Checks the token; returns the e-mail address it belongs to.
pub fn verify(token: &str, project: &str, allowed: &[String]) -> Result<String, String> {
    let kid = decode_header(token).map_err(|_| "The sign-in token is not valid.")?.kid.ok_or("The sign-in token has no key id.")?;
    let keys = google_keys()?;
    let jwk = keys.get("keys").and_then(Value::as_array).and_then(|a| a.iter().find(|k| k.get("kid").and_then(Value::as_str) == Some(&kid))).ok_or("The sign-in token was signed with an unknown key.")?;
    let (n, e) = (jwk.get("n").and_then(Value::as_str).unwrap_or(""), jwk.get("e").and_then(Value::as_str).unwrap_or(""));
    let key = DecodingKey::from_rsa_components(n, e).map_err(|e| e.to_string())?;
    let mut v = Validation::new(Algorithm::RS256);
    v.set_audience(&[project]);
    v.set_issuer(&[format!("https://securetoken.google.com/{project}")]);
    let claims = decode::<Value>(token, &key, &v).map_err(|e| format!("The sign-in is not accepted: {e}"))?.claims;
    let email = claims.get("email").and_then(Value::as_str).unwrap_or("").to_lowercase();
    let verified = claims.get("email_verified").and_then(Value::as_bool).unwrap_or(false);
    if email.is_empty() || !verified || !allowed.iter().any(|a| a.eq_ignore_ascii_case(&email)) {
        return Err("This account is not allowed to use this ZO-AI.".into());
    }
    Ok(email)
}
