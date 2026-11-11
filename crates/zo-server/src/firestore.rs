//! A very small Firestore client (REST), signed in as the project's service account. Every document holds one text (`data`);
//! a text too long for one document is cut into parts: `name`, `name__1`, `name__2`, ...

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Firestore refuses documents over 1 MiB; stay well under.
pub const PART: usize = 700_000;

pub struct Firestore {
    project: String,
    email: String,
    key: EncodingKey,
    token_uri: String,
    token: Mutex<Option<(u64, String)>>,
    /// Everything lives under this document, for example `zoai/owner`.
    root: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Document ids may not contain a slash and must not be empty.
pub fn doc_id(s: &str) -> String {
    let id: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).take(200).collect();
    if id.is_empty() || id.starts_with("__") { format!("x{id}") } else { id }
}

/// Cuts a text into parts of at most `max` bytes, never inside a character.
pub fn split(text: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while rest.len() > max {
        let mut cut = max;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    out.push(rest);
    out
}

/// Puts the parts of each text back together: `{"a": .., "a__1": ..}` becomes `{"a": whole}`.
pub fn join(docs: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut whole: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    for (id, text) in docs {
        let (name, n) = match id.rsplit_once("__") {
            Some((name, n)) if n.parse::<usize>().is_ok() => (name.to_string(), n.parse().unwrap()),
            _ => (id, 0),
        };
        whole.entry(name).or_default().push((n, text));
    }
    whole
        .into_iter()
        .map(|(name, mut parts)| {
            parts.sort_by_key(|p| p.0);
            (name, parts.into_iter().map(|p| p.1).collect::<String>())
        })
        .collect()
}

impl Firestore {
    /// `account` is the service account key (the JSON file from the Firebase console).
    pub fn new(account: &str, root: &str) -> Result<Firestore, String> {
        let v: Value = serde_json::from_str(account).map_err(|_| "FIREBASE_SERVICE_ACCOUNT is not the JSON key file.")?;
        let get = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string).ok_or(format!("The service account key has no {k}."));
        Ok(Firestore {
            project: get("project_id")?,
            email: get("client_email")?,
            key: EncodingKey::from_rsa_pem(get("private_key")?.as_bytes()).map_err(|e| format!("The service account's private key cannot be read: {e}"))?,
            token_uri: get("token_uri").unwrap_or_else(|_| "https://oauth2.googleapis.com/token".into()),
            token: Mutex::new(None),
            root: root.trim_matches('/').to_string(),
        })
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    fn token(&self) -> Result<String, String> {
        let mut g = self.token.lock().unwrap();
        if let Some((until, t)) = g.as_ref() {
            if *until > now() + 60 {
                return Ok(t.clone());
            }
        }
        let iat = now();
        let claims = json!({"iss": self.email, "scope": "https://www.googleapis.com/auth/datastore", "aud": self.token_uri, "iat": iat, "exp": iat + 3600});
        let jwt = encode(&Header::new(Algorithm::RS256), &claims, &self.key).map_err(|e| e.to_string())?;
        let r: Value = ureq::post(&self.token_uri)
            .timeout(Duration::from_secs(20))
            .send_form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", &jwt)])
            .map_err(|e| format!("Google did not accept the service account: {e}"))?
            .into_json()
            .map_err(|e| e.to_string())?;
        let t = r.get("access_token").and_then(Value::as_str).ok_or("Google gave no access token.")?.to_string();
        *g = Some((iat + r.get("expires_in").and_then(Value::as_u64).unwrap_or(3600), t.clone()));
        Ok(t)
    }

    fn base(&self) -> String {
        format!("https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/{}", self.project, self.root)
    }

    fn put_one(&self, collection: &str, id: &str, text: &str) -> Result<(), String> {
        let body = json!({"fields": {"data": {"stringValue": text}, "updated": {"integerValue": now().to_string()}}});
        ureq::request("PATCH", &format!("{}/{collection}/{id}", self.base()))
            .timeout(Duration::from_secs(30))
            .set("Authorization", &format!("Bearer {}", self.token()?))
            .send_json(body)
            .map_err(|e| format!("Firestore refused to save {collection}/{id}: {e}"))?;
        Ok(())
    }

    fn delete_one(&self, collection: &str, id: &str) -> Result<(), String> {
        match ureq::delete(&format!("{}/{collection}/{id}", self.base())).timeout(Duration::from_secs(30)).set("Authorization", &format!("Bearer {}", self.token()?)).call() {
            Ok(_) | Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(e) => Err(format!("Firestore refused to delete {collection}/{id}: {e}")),
        }
    }

    /// Saves one text under a name. `old_parts` is how many documents it took last time (extra ones are removed).
    pub fn put(&self, collection: &str, name: &str, text: &str, old_parts: usize) -> Result<usize, String> {
        let parts = split(text, PART);
        for (n, p) in parts.iter().enumerate() {
            let id = if n == 0 { name.to_string() } else { format!("{name}__{n}") };
            self.put_one(collection, &id, p)?;
        }
        for n in parts.len()..old_parts {
            self.delete_one(collection, &format!("{name}__{n}"))?;
        }
        Ok(parts.len())
    }

    pub fn delete(&self, collection: &str, name: &str, parts: usize) -> Result<(), String> {
        self.delete_one(collection, name)?;
        for n in 1..parts {
            self.delete_one(collection, &format!("{name}__{n}"))?;
        }
        Ok(())
    }

    /// Everything in one collection: name -> (text, how many documents it takes).
    pub fn all(&self, collection: &str) -> Result<BTreeMap<String, (String, usize)>, String> {
        let mut docs = BTreeMap::new();
        let mut page = String::new();
        loop {
            let mut req = ureq::get(&format!("{}/{collection}", self.base())).timeout(Duration::from_secs(60)).set("Authorization", &format!("Bearer {}", self.token()?)).query("pageSize", "300");
            if !page.is_empty() {
                req = req.query("pageToken", &page);
            }
            let r: Value = match req.call() {
                Ok(r) => r.into_json().map_err(|e| e.to_string())?,
                Err(ureq::Error::Status(404, _)) => break,
                Err(e) => return Err(format!("Firestore could not list {collection}: {e}")),
            };
            for d in r.get("documents").and_then(Value::as_array).into_iter().flatten() {
                let id = d.get("name").and_then(Value::as_str).and_then(|n| n.rsplit('/').next()).unwrap_or("").to_string();
                let text = d.pointer("/fields/data/stringValue").and_then(Value::as_str).unwrap_or("").to_string();
                docs.insert(id, text);
            }
            page = r.get("nextPageToken").and_then(Value::as_str).unwrap_or("").to_string();
            if page.is_empty() {
                break;
            }
        }
        let mut count: BTreeMap<String, usize> = BTreeMap::new();
        for id in docs.keys() {
            let name = match id.rsplit_once("__") {
                Some((name, n)) if n.parse::<usize>().is_ok() => name,
                _ => id.as_str(),
            };
            *count.entry(name.to_string()).or_default() += 1;
        }
        Ok(join(docs).into_iter().map(|(name, text)| { let n = count.get(&name).copied().unwrap_or(1); (name, (text, n)) }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_texts_are_cut_and_joined_again() {
        let text = "aé".repeat(1000);
        let parts = split(&text, 301);
        assert!(parts.len() > 1 && parts.iter().all(|p| p.len() <= 301));
        let mut docs = BTreeMap::new();
        for (n, p) in parts.iter().enumerate() {
            docs.insert(if n == 0 { "c1".to_string() } else { format!("c1__{n}") }, p.to_string());
        }
        docs.insert("other".into(), "x".into());
        let whole = join(docs);
        assert_eq!(whole["c1"], text);
        assert_eq!(whole["other"], "x");
        assert_eq!(whole.len(), 2);
    }

    #[test]
    fn ids_are_safe() {
        assert_eq!(doc_id("a/b c"), "a_b_c");
        assert_eq!(doc_id(""), "x");
        assert_eq!(doc_id("__1"), "x__1");
    }
}
