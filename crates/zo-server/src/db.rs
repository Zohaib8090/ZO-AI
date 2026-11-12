//! The database the server keeps its users' data in: Firestore in production, an in-memory fake in the tests. A collection is a
//! path (`users/<tenant>/chats`, `registry`, `deviceidx`); every document holds one text. Long texts are cut into parts (see
//! `firestore::encode_parts`), which is done here, on top of the plain document calls.

use crate::firestore::{encode_parts, join, Joined};
use std::collections::BTreeMap;

pub trait Db: Send + Sync {
    /// Saves (or replaces) one document.
    fn put_doc(&self, collection: &str, id: &str, text: &str) -> Result<(), String>;
    fn get_doc(&self, collection: &str, id: &str) -> Result<Option<String>, String>;
    /// Removing a document that is not there is not an error.
    fn delete_doc(&self, collection: &str, id: &str) -> Result<(), String>;
    /// Every document of a collection: id -> text.
    fn list_docs(&self, collection: &str) -> Result<BTreeMap<String, String>, String>;
    /// The names of the collections directly below a document, for example below `users/<tenant>`.
    fn sub_collections(&self, parent: &str) -> Result<Vec<String>, String>;
    /// Removes the person's login (their Firebase Authentication user), so Firebase can purge it. Ok(false): this database has no
    /// such thing (the tests).
    fn delete_login(&self, _uid: &str) -> Result<bool, String> {
        Ok(false)
    }
    /// For Firebase Cloud Messaging: the project id and an access token (scope firebase.messaging) of the service account. None: this
    /// database has no such thing (the tests, which give the push module a fixed one).
    fn fcm_access(&self) -> Option<Result<(String, String), String>> {
        None
    }
}

/// Saves one text under a name. `old_parts` is how many documents it may take up now (extra ones are removed). The later parts are
/// written first and the first part, which carries the mark, last: a reader that finds the new first part finds all its parts.
pub fn put_text(db: &dyn Db, collection: &str, name: &str, text: &str, old_parts: usize) -> Result<usize, String> {
    let parts = encode_parts(text);
    for n in (0..parts.len()).rev() {
        let id = if n == 0 { name.to_string() } else { format!("{name}__{n}") };
        db.put_doc(collection, &id, &parts[n])?;
    }
    for n in parts.len()..old_parts {
        db.delete_doc(collection, &format!("{name}__{n}"))?;
    }
    Ok(parts.len())
}

pub fn delete_text(db: &dyn Db, collection: &str, name: &str, parts: usize) -> Result<(), String> {
    db.delete_doc(collection, name)?;
    for n in 1..parts {
        db.delete_doc(collection, &format!("{name}__{n}"))?;
    }
    Ok(())
}

/// Everything in one collection: name -> the text, how many documents it takes, and whether it can be trusted.
pub fn all_texts(db: &dyn Db, collection: &str) -> Result<BTreeMap<String, Joined>, String> {
    Ok(join(db.list_docs(collection)?))
}

/// Removes every document below `users/<tenant>`: the collections the server writes, plus any other the database reports.
/// Safe to run again after a failure. Returns how many documents were removed.
pub fn wipe_tree(db: &dyn Db, root: &str, known: &[&str]) -> Result<usize, String> {
    let mut names: Vec<String> = known.iter().map(|s| s.to_string()).collect();
    for c in db.sub_collections(root)? {
        if !names.contains(&c) {
            names.push(c);
        }
    }
    let mut n = 0;
    for c in names {
        let col = format!("{root}/{c}");
        for id in db.list_docs(&col)?.into_keys() {
            db.delete_doc(&col, &id)?;
            n += 1;
        }
    }
    Ok(n)
}

/// A database in memory, for the tests. It can be told to fail, to see how the server behaves when Firestore does.
#[cfg(test)]
#[derive(Default)]
pub struct MemDb {
    pub docs: std::sync::Mutex<BTreeMap<(String, String), String>>,
    pub fail: std::sync::atomic::AtomicBool,
    pub writes: std::sync::atomic::AtomicUsize,
    /// How many times a whole collection was listed.
    pub reads: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl MemDb {
    fn check(&self) -> Result<(), String> {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            Err("the test database is down".into())
        } else {
            Ok(())
        }
    }

    /// How many documents live at or below a path prefix.
    pub fn count_under(&self, prefix: &str) -> usize {
        self.docs.lock().unwrap().keys().filter(|(c, _)| c == prefix || c.starts_with(&format!("{prefix}/"))).count()
    }
}

#[cfg(test)]
impl Db for MemDb {
    fn put_doc(&self, collection: &str, id: &str, text: &str) -> Result<(), String> {
        self.check()?;
        self.writes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.docs.lock().unwrap().insert((collection.into(), id.into()), text.into());
        Ok(())
    }
    fn get_doc(&self, collection: &str, id: &str) -> Result<Option<String>, String> {
        self.check()?;
        Ok(self.docs.lock().unwrap().get(&(collection.to_string(), id.to_string())).cloned())
    }
    fn delete_doc(&self, collection: &str, id: &str) -> Result<(), String> {
        self.check()?;
        self.docs.lock().unwrap().remove(&(collection.to_string(), id.to_string()));
        Ok(())
    }
    fn list_docs(&self, collection: &str) -> Result<BTreeMap<String, String>, String> {
        self.check()?;
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.docs.lock().unwrap().iter().filter(|((c, _), _)| c == collection).map(|((_, i), t)| (i.clone(), t.clone())).collect())
    }
    fn sub_collections(&self, parent: &str) -> Result<Vec<String>, String> {
        self.check()?;
        let pre = format!("{parent}/");
        let mut v: Vec<String> = self.docs.lock().unwrap().keys().filter_map(|(c, _)| c.strip_prefix(&pre).map(|r| r.split('/').next().unwrap_or("").to_string())).collect();
        v.sort();
        v.dedup();
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_roundtrip_through_parts_and_wipe_removes_everything_below_a_root() {
        let db = MemDb::default();
        let big = "x".repeat(crate::firestore::PART + 5);
        assert_eq!(put_text(&db, "users/a/chats", "c1", &big, 0).unwrap(), 2);
        put_text(&db, "users/a/odd", "z", "1", 0).unwrap();
        put_text(&db, "users/b/chats", "c1", "keep", 0).unwrap();
        assert_eq!(all_texts(&db, "users/a/chats").unwrap()["c1"].text, big);
        let n = wipe_tree(&db, "users/a", &["chats", "memory", "files", "meta"]).unwrap();
        assert_eq!(n, 3);
        assert_eq!(db.count_under("users/a"), 0);
        assert_eq!(db.count_under("users/b"), 1, "another user is untouched");
        assert_eq!(wipe_tree(&db, "users/a", &["chats"]).unwrap(), 0, "running it again is harmless");
    }
}
