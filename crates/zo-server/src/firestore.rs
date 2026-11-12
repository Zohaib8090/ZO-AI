//! A very small Firestore client (REST), signed in as the project's service account. Every document holds one text (`data`);
//! a text too long for one document is cut into parts: `name`, `name__1`, `name__2`, ... (the cutting lives in `db.rs`, on top of
//! the `Db` trait this client implements, so the same code runs against an in-memory fake in the tests).

use crate::db::Db;
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
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Document ids may not contain a slash and must not be empty.
pub fn doc_id(s: &str) -> String {
    let id: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).take(200).collect();
    // "." and ".." are not document ids in Firestore (and would change the path)
    if id.is_empty() || id.starts_with("__") || id.starts_with('.') { format!("x{id}") } else { id }
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

/// Marks the first part of a saved text: how many parts there are and a fingerprint of the whole text. A reader that finds
/// parts from two different saves (a write was cut short) sees that the fingerprint does not fit, instead of reading a mix.
const MARK: char = '\u{1}';

fn fingerprint(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The documents a text is saved as: the parts, the first one with its mark in front.
pub fn encode_parts(text: &str) -> Vec<String> {
    let parts = split(text, PART);
    let head = format!("{MARK}zo1:{}:{}{MARK}", parts.len(), fingerprint(text));
    parts.iter().enumerate().map(|(n, p)| if n == 0 { format!("{head}{p}") } else { p.to_string() }).collect()
}

/// One saved text read back.
#[derive(Debug, PartialEq)]
pub struct Joined {
    pub text: String,
    /// How many documents it takes now (stale ones included, so that they are cleaned up).
    pub parts: usize,
    /// False when the first part is missing, parts are missing, or the fingerprint does not fit: the text must not be used.
    pub ok: bool,
}

/// Puts the parts of each text back together: `{"a": .., "a__1": ..}` becomes `{"a": whole}`.
pub fn join(docs: BTreeMap<String, String>) -> BTreeMap<String, Joined> {
    let mut whole: BTreeMap<String, BTreeMap<usize, String>> = BTreeMap::new();
    for (id, text) in docs {
        let (name, n) = match id.rsplit_once("__") {
            Some((name, n)) if n.parse::<usize>().is_ok() => (name.to_string(), n.parse().unwrap()),
            _ => (id, 0),
        };
        whole.entry(name).or_default().insert(n, text);
    }
    whole
        .into_iter()
        .map(|(name, parts)| {
            let total = parts.len();
            // the parts that follow each other from 0
            let run: Vec<&String> = (0..).map_while(|n| parts.get(&n)).collect();
            let joined = match run.first().and_then(|h| h.strip_prefix(MARK)) {
                Some(rest) => {
                    // `zo1:<parts>:<fingerprint>` MARK first-part-text
                    let parsed = rest.split_once(MARK).and_then(|(h, first)| {
                        let mut f = h.strip_prefix("zo1:")?.splitn(2, ':');
                        let (n, fp): (usize, &str) = (f.next()?.parse().ok()?, f.next()?);
                        (n >= 1 && run.len() >= n).then(|| (n, fp.to_string(), first))
                    });
                    parsed.and_then(|(n, fp, first)| {
                        let text: String = std::iter::once(first).chain(run[1..n].iter().map(|s| s.as_str())).collect();
                        (fingerprint(&text) == fp).then_some(text)
                    })
                }
                // saved before the mark existed: the parts as they are
                None => (!run.is_empty() && run.len() == total).then(|| run.iter().map(|s| s.as_str()).collect::<String>()),
            };
            (name, Joined { ok: joined.is_some(), text: joined.unwrap_or_default(), parts: total })
        })
        .collect()
}

impl Firestore {
    /// `account` is the service account key (the JSON file from the Firebase console).
    pub fn new(account: &str) -> Result<Firestore, String> {
        let v: Value = serde_json::from_str(account).map_err(|_| "FIREBASE_SERVICE_ACCOUNT is not the JSON key file.")?;
        let get = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string).ok_or(format!("The service account key has no {k}."));
        Ok(Firestore {
            project: get("project_id")?,
            email: get("client_email")?,
            key: EncodingKey::from_rsa_pem(get("private_key")?.as_bytes()).map_err(|e| format!("The service account's private key cannot be read: {e}"))?,
            token_uri: get("token_uri").unwrap_or_else(|_| "https://oauth2.googleapis.com/token".into()),
            token: Mutex::new(None),
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
        let claims = json!({"iss": self.email, "scope": "https://www.googleapis.com/auth/datastore https://www.googleapis.com/auth/identitytoolkit", "aud": self.token_uri, "iat": iat, "exp": iat + 3600});
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
        format!("https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents", self.project)
    }
}

impl Db for Firestore {
    fn put_doc(&self, collection: &str, id: &str, text: &str) -> Result<(), String> {
        let body = json!({"fields": {"data": {"stringValue": text}, "updated": {"integerValue": now().to_string()}}});
        ureq::request("PATCH", &format!("{}/{collection}/{id}", self.base()))
            .timeout(Duration::from_secs(30))
            .set("Authorization", &format!("Bearer {}", self.token()?))
            .send_json(body)
            .map_err(|e| format!("Firestore refused to save {collection}/{id}: {e}"))?;
        Ok(())
    }

    fn get_doc(&self, collection: &str, id: &str) -> Result<Option<String>, String> {
        match ureq::get(&format!("{}/{collection}/{id}", self.base())).timeout(Duration::from_secs(30)).set("Authorization", &format!("Bearer {}", self.token()?)).call() {
            Ok(r) => {
                let v: Value = r.into_json().map_err(|e| e.to_string())?;
                Ok(Some(v.pointer("/fields/data/stringValue").and_then(Value::as_str).unwrap_or("").to_string()))
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(format!("Firestore could not read {collection}/{id}: {e}")),
        }
    }

    fn delete_doc(&self, collection: &str, id: &str) -> Result<(), String> {
        match ureq::delete(&format!("{}/{collection}/{id}", self.base())).timeout(Duration::from_secs(30)).set("Authorization", &format!("Bearer {}", self.token()?)).call() {
            Ok(_) | Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(e) => Err(format!("Firestore refused to delete {collection}/{id}: {e}")),
        }
    }

    fn list_docs(&self, collection: &str) -> Result<BTreeMap<String, String>, String> {
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
        Ok(docs)
    }

    fn delete_login(&self, uid: &str) -> Result<bool, String> {
        match ureq::post(&format!("https://identitytoolkit.googleapis.com/v1/projects/{}/accounts:delete", self.project))
            .timeout(Duration::from_secs(30))
            .set("Authorization", &format!("Bearer {}", self.token()?))
            .send_json(json!({"localId": uid}))
        {
            Ok(_) => Ok(true),
            Err(ureq::Error::Status(400, r)) => {
                // already gone: the same result
                if r.into_string().unwrap_or_default().contains("USER_NOT_FOUND") {
                    Ok(true)
                } else {
                    Err("Firebase Authentication refused to remove the login.".to_string())
                }
            }
            Err(e) => Err(format!("Firebase Authentication refused to remove the login: {e}")),
        }
    }

    fn sub_collections(&self, parent: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        let mut page = String::new();
        loop {
            let mut body = json!({"pageSize": 100});
            if !page.is_empty() {
                body["pageToken"] = json!(page);
            }
            let r: Value = match ureq::post(&format!("{}/{parent}:listCollectionIds", self.base()))
                .timeout(Duration::from_secs(30))
                .set("Authorization", &format!("Bearer {}", self.token()?))
                .send_json(body)
            {
                Ok(r) => r.into_json().map_err(|e| e.to_string())?,
                Err(ureq::Error::Status(404, _)) => break,
                Err(e) => return Err(format!("Firestore could not list the collections of {parent}: {e}")),
            };
            out.extend(r.get("collectionIds").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string));
            page = r.get("nextPageToken").and_then(Value::as_str).unwrap_or("").to_string();
            if page.is_empty() {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(texts: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut docs = BTreeMap::new();
        for (name, text) in texts {
            for (n, p) in encode_parts(text).into_iter().enumerate() {
                docs.insert(if n == 0 { name.to_string() } else { format!("{name}__{n}") }, p);
            }
        }
        docs
    }

    #[test]
    fn long_texts_are_cut_and_joined_again() {
        let text = "aé".repeat(1000);
        let parts = split(&text, 301);
        assert!(parts.len() > 1 && parts.iter().all(|p| p.len() <= 301));
        let big = "xé".repeat(PART);
        assert!(encode_parts(&big).len() > 1);
        let docs = stored(&[("c1", &big), ("other", "x")]);
        let whole = join(docs);
        assert_eq!(whole["c1"].text, big);
        assert!(whole["c1"].ok && whole["c1"].parts == encode_parts(&big).len());
        assert_eq!((whole["other"].text.as_str(), whole["other"].ok, whole["other"].parts), ("x", true, 1));
        assert_eq!(whole.len(), 2);
    }

    #[test]
    fn a_text_saved_by_an_older_version_is_still_read() {
        let mut docs = BTreeMap::new();
        docs.insert("c1".to_string(), "{\"a\":".to_string());
        docs.insert("c1__1".to_string(), "1}".to_string());
        docs.insert("c2".to_string(), "{}".to_string());
        let j = join(docs);
        assert_eq!((j["c1"].text.as_str(), j["c1"].ok), ("{\"a\":1}", true));
        assert_eq!((j["c2"].text.as_str(), j["c2"].ok), ("{}", true));
    }

    #[test]
    fn a_cut_short_or_mixed_save_is_noticed_not_read() {
        let big = "y".repeat(PART + 10);
        let small = "z".repeat(5);
        // the later part of a new save was written, but the old first part is still there
        let mut docs = stored(&[("c", &small)]);
        docs.insert("c__1".into(), "tail".into());
        let j = join(docs);
        assert!(j["c"].ok, "a stray part after a one-part save is ignored, not mixed in");
        assert_eq!((j["c"].text.as_str(), j["c"].parts), (small.as_str(), 2), "and it is counted, so the next save removes it");
        // old first part (1 part) replaced by a bigger save's tail only: first part says 2 parts, tail is from another save
        let mut docs = stored(&[("c", &big)]);
        docs.insert("c__1".into(), "another save".into());
        assert!(!join(docs)["c"].ok);
        // the first part is missing
        let mut docs = stored(&[("c", &big)]);
        docs.remove("c");
        let j = join(docs);
        assert!(!j["c"].ok && j["c"].text.is_empty());
        // a part is missing in the middle
        let huge = "q".repeat(PART * 2 + 1);
        let mut docs = stored(&[("c", &huge)]);
        docs.remove("c__1");
        assert!(!join(docs)["c"].ok);
    }

    #[test]
    fn ids_are_safe() {
        assert_eq!(doc_id("a/b c"), "a_b_c");
        assert_eq!(doc_id(""), "x");
        assert_eq!(doc_id("__1"), "x__1");
    }

    #[test]
    fn document_ids_are_never_dot_names() {
        for bad in [".", "..", "", "__x", ".hidden"] {
            let id = doc_id(bad);
            assert!(id != "." && id != ".." && !id.is_empty() && !id.starts_with("__") && !id.starts_with('.'), "{bad:?} -> {id:?}");
        }
        assert_eq!(doc_id("c-123_abc"), "c-123_abc");
    }
}
