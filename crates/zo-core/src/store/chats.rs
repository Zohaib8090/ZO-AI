//! Saved chats: what the sidebar lists under Recents, and what opens again when one is clicked.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use crate::ai::wire::Turn;

/// One take of the rest of a conversation from a message on (what came after your message, as it was when you asked again).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Version {
    pub entries: Vec<Entry>,
    pub history: Vec<Turn>,
    /// The other forks that were made later inside this take.
    #[serde(default)]
    pub sub: BTreeMap<usize, Branch>,
}

/// The takes of one message: asking again (retry) or editing it makes a new one; the arrows switch between them.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Branch {
    pub versions: Vec<Version>,
    pub current: usize,
}

/// One line of what the person sees in a chat. `kind` is the style it is shown in.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub updated: u64,
    pub entries: Vec<Entry>,
    /// What the API providers need to continue the conversation.
    #[serde(default)]
    pub history: Vec<Turn>,
    /// Claude Code mode keeps its own conversation under this id.
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub pinned: bool,
    /// By the number of your message (counting from 0): the takes it has had.
    #[serde(default)]
    pub branches: BTreeMap<usize, Branch>,
}

const KEEP: usize = 100;

fn path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into())).join(".local/share")
    });
    base.join("zo-ai").join("chats.json")
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Newest first.
pub fn load() -> Vec<Chat> {
    let mut v: Vec<Chat> = std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.sort_by(|a, b| b.updated.cmp(&a.updated));
    v
}

fn write(all: &[Chat]) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("json.tmp");
    if let Ok(s) = serde_json::to_string(all) {
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// Adds the chat, or replaces the one with the same id.
pub fn upsert(mut chat: Chat) {
    let mut all = load();
    // pinning is only changed by `set_pinned`, so saving a chat never loses it
    chat.pinned = all.iter().find(|c| c.id == chat.id).is_some_and(|c| c.pinned);
    all.retain(|c| c.id != chat.id);
    all.insert(0, chat);
    all.truncate(KEEP);
    write(&all);
}

/// Pins or unpins a chat. Pinned chats are listed first and keep their order by the last message.
pub fn set_pinned(id: &str, on: bool) {
    let mut all = load();
    if let Some(c) = all.iter_mut().find(|c| c.id == id) {
        c.pinned = on;
    }
    write(&all);
}

pub fn delete(id: &str) {
    let mut all = load();
    all.retain(|c| c.id != id);
    write(&all);
}

/// A reply as plain words: the marks of bold, code, headings and links taken out, for showing a short preview.
pub fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("](") {
        let Some(j) = rest[i..].find(')') else { break };
        out.push_str(&rest[..i]);
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out.replace(['*', '`', '#', '['], "").replace("\n", " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One search result.
pub struct Hit {
    pub id: String,
    pub title: String,
    /// A few words around the match.
    pub snippet: String,
    pub updated: u64,
}

/// About 90 characters around the first match of `needle` (already lower case) in `text`.
fn snippet_around(text: &str, needle: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    if lower.len() != chars.len() {
        return text.to_lowercase().contains(needle).then(|| chars.iter().take(90).collect());
    }
    let n: Vec<char> = needle.chars().collect();
    let at = (0..lower.len().saturating_sub(n.len().saturating_sub(1))).find(|&i| lower[i..i + n.len()] == n[..])?;
    let start = at.saturating_sub(30);
    let end = (at + n.len() + 60).min(chars.len());
    let mut s: String = chars[start..end].iter().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    if start > 0 {
        s = format!("…{s}");
    }
    if end < chars.len() {
        s.push('…');
    }
    Some(plain(&s))
}

/// Chats whose title or text holds `query`, newest first. An empty query lists them all.
pub fn search(query: &str, all: &[Chat]) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    let mut hits: Vec<Hit> = all.iter().filter_map(|c| {
        if q.is_empty() {
            let first = c.entries.iter().find(|e| e.kind == "msg-ai").or(c.entries.first());
            return Some(Hit { id: c.id.clone(), title: c.title.clone(), snippet: first.map(|e| plain(&e.text).chars().take(90).collect::<String>()).unwrap_or_default(), updated: c.updated });
        }
        if let Some(s) = snippet_around(&c.title, &q) {
            let _ = s;
            let body = c.entries.iter().find(|e| e.kind == "msg-ai").map(|e| plain(&e.text).chars().take(90).collect::<String>()).unwrap_or_default();
            return Some(Hit { id: c.id.clone(), title: c.title.clone(), snippet: body, updated: c.updated });
        }
        c.entries.iter().filter(|e| e.kind == "msg-user" || e.kind == "msg-ai").find_map(|e| snippet_around(&e.text, &q))
            .map(|snippet| Hit { id: c.id.clone(), title: c.title.clone(), snippet, updated: c.updated })
    }).collect();
    hits.sort_by(|a, b| b.updated.cmp(&a.updated));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chats_are_saved_replaced_listed_newest_first_and_deleted() {
        let _g = super::super::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-chats-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", &dir);
        let mk = |id: &str, t: u64| Chat { id: id.into(), title: format!("chat {id}"), updated: t, ..Default::default() };
        upsert(mk("a", 1));
        upsert(mk("b", 2));
        upsert(Chat { title: "renamed".into(), ..mk("a", 3) });
        let all = load();
        assert_eq!(all.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(all[0].title, "renamed");
        let found = search("RENAMED", &load());
        assert_eq!(found.len(), 1);
        set_pinned("b", true);
        upsert(Chat { title: "b again".into(), ..mk("b", 9) });
        assert!(load().iter().find(|c| c.id == "b").unwrap().pinned, "saving a chat keeps it pinned");
        assert_eq!(load()[0].id, "b", "newest first");
        set_pinned("b", false);
        assert!(!load().iter().any(|c| c.pinned));
        delete("a");
        assert_eq!(load().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_DATA_HOME");
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    fn chat(id: &str, title: &str, updated: u64, lines: &[(&str, &str)]) -> Chat {
        Chat { id: id.into(), title: title.into(), updated, entries: lines.iter().map(|(k, t)| Entry { kind: k.to_string(), text: t.to_string() }).collect(), ..Default::default() }
    }

    #[test]
    fn finds_by_title_and_by_text_with_a_snippet_newest_first() {
        let all = vec![
            chat("1", "disk space", 1, &[("msg-user", "how full is my disk"), ("msg-ai", "Your root partition is 89% full, mostly in /var/lib.")]),
            chat("2", "photos", 3, &[("msg-user", "find big pictures"), ("msg-ai", "Found 12 files over 20 MB.")]),
            chat("3", "other", 2, &[("msg-user", "what is the DISK speed"), ("msg-tool", "disk hidden in a tool line")]),
        ];
        let hits = search("disk", &all);
        assert_eq!(hits.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["3", "1"], "newest first, only chats that mention it");
        assert!(hits[0].snippet.to_lowercase().contains("disk"));
        assert!(search("zzz", &all).is_empty());
        assert_eq!(search("", &all).len(), 3, "an empty search lists everything");
    }

    #[test]
    fn previews_drop_the_marks_of_markdown() {
        assert_eq!(plain("# A heading\n\nSome **bold** and `code` plus a [link](https://example.com/a)."), "A heading Some bold and code plus a link.");
    }

    #[test]
    fn snippets_cut_around_the_match_and_survive_unicode() {
        let long = format!("{} needle {}", "a".repeat(100), "b".repeat(100));
        let s = snippet_around(&long, "needle").unwrap();
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("needle") && s.chars().count() < 110);
        assert!(snippet_around("Größe ändern ÄÖÜ", "ändern").is_some());
        assert_eq!(snippet_around("short", "x"), None);
    }
}

// ---- takes: asking again or editing a message keeps what was there as an earlier take ----------------------------------------

impl Chat {
    /// The entry and turn at which your message number `nth` (counting from 0) starts.
    pub fn tail_start(&self, nth: usize) -> Option<(usize, usize)> {
        let at = self.entries.iter().enumerate().filter(|(_, e)| e.kind == "msg-user").map(|(i, _)| i).nth(nth)?;
        let turn_at = self.history.iter().enumerate().filter(|(_, t)| matches!(t, Turn::User(_) | Turn::UserImages(..))).map(|(i, _)| i).nth(nth).unwrap_or(self.history.len());
        Some((at, turn_at))
    }

    /// Keeps the conversation from message `nth` on as a take of its own, opens a new empty one in its place, and cuts the chat there.
    pub fn fork_at(&mut self, nth: usize) -> Option<String> {
        let (at, turn_at) = self.tail_start(nth)?;
        let text = self.entries[at].text.clone();
        let later: Vec<usize> = self.branches.range(nth + 1..).map(|(k, _)| *k).collect();
        let sub: BTreeMap<usize, Branch> = later.into_iter().filter_map(|k| self.branches.remove(&k).map(|b| (k, b))).collect();
        let tail = Version { entries: self.entries.get(at..).map(<[_]>::to_vec).unwrap_or_default(), history: self.history.get(turn_at..).map(<[_]>::to_vec).unwrap_or_default(), sub };
        let b = self.branches.entry(nth).or_insert_with(|| Branch { versions: vec![Version::default()], current: 0 });
        let cur = b.current.min(b.versions.len() - 1);
        b.versions[cur] = tail;
        b.versions.push(Version::default());
        b.current = b.versions.len() - 1;
        let is_last = self.entries.iter().filter(|e| e.kind == "msg-user").count() == nth + 1;
        self.entries.truncate(at);
        self.history.truncate(turn_at);
        if !is_last {
            // Claude Code's own memory of the conversation cannot go back
            self.session = None;
        }
        Some(text)
    }

    /// The takes as they should be stored: the one on show is updated with what is there now.
    pub fn snapshot_branches(&self) -> BTreeMap<usize, Branch> {
        let mut snap = self.branches.clone();
        let keys: Vec<usize> = snap.keys().copied().collect();
        for k in keys.into_iter().rev() {
            let Some((at, turn_at)) = self.tail_start(k) else { continue };
            let sub: BTreeMap<usize, Branch> = snap.range(k + 1..).map(|(i, b)| (*i, b.clone())).collect();
            let tail = Version { entries: self.entries[at..].to_vec(), history: self.history.get(turn_at..).map(<[_]>::to_vec).unwrap_or_default(), sub };
            if let Some(b) = snap.get_mut(&k) {
                let cur = b.current.min(b.versions.len().saturating_sub(1));
                if cur < b.versions.len() {
                    b.versions[cur] = tail;
                }
            }
        }
        snap
    }

    /// Shows an other take of your message number `nth`: `delta` is -1 for the one before and 1 for the one after.
    pub fn switch_version(&mut self, nth: usize, delta: i32) -> bool {
        let Some((at, turn_at)) = self.tail_start(nth) else { return false };
        let (cur, count) = match self.branches.get(&nth) {
            Some(b) => (b.current, b.versions.len()),
            None => return false,
        };
        let next = cur as i32 + delta;
        if next < 0 || next as usize >= count {
            return false;
        }
        let next = next as usize;
        let later: Vec<usize> = self.branches.range(nth + 1..).map(|(k, _)| *k).collect();
        let sub: BTreeMap<usize, Branch> = later.into_iter().filter_map(|k| self.branches.remove(&k).map(|b| (k, b))).collect();
        let outgoing = Version { entries: self.entries[at..].to_vec(), history: self.history.get(turn_at..).map(<[_]>::to_vec).unwrap_or_default(), sub };
        let b = self.branches.get_mut(&nth).expect("checked above");
        b.versions[cur] = outgoing;
        let incoming = b.versions[next].clone();
        b.current = next;
        self.branches.extend(incoming.sub);
        self.entries.truncate(at);
        self.entries.extend(incoming.entries);
        self.history.truncate(turn_at);
        self.history.extend(incoming.history);
        self.session = None;
        true
    }

    /// For each of your messages that has had more than one take: (message number, shown take, how many takes).
    pub fn take_counts(&self) -> Vec<(usize, usize, usize)> {
        self.branches.iter().filter(|(_, b)| b.versions.len() > 1).map(|(k, b)| (*k, b.current, b.versions.len())).collect()
    }
}

#[cfg(test)]
mod take_tests {
    use super::*;

    fn chat() -> Chat {
        let e = |k: &str, t: &str| Entry { kind: k.into(), text: t.into() };
        Chat {
            id: "c".into(),
            entries: vec![e("msg-user", "one"), e("msg-ai", "a1"), e("msg-user", "two"), e("msg-ai", "a2")],
            history: vec![Turn::User("one".into()), Turn::Assistant { text: "a1".into(), calls: vec![] }, Turn::User("two".into()), Turn::Assistant { text: "a2".into(), calls: vec![] }],
            ..Default::default()
        }
    }

    #[test]
    fn editing_keeps_the_old_take_and_switching_brings_it_back() {
        let mut c = chat();
        assert_eq!(c.fork_at(1).as_deref(), Some("two"));
        assert_eq!(c.entries.len(), 2);
        // the new take is answered
        c.entries.push(Entry { kind: "msg-user".into(), text: "two, edited".into() });
        c.entries.push(Entry { kind: "msg-ai".into(), text: "a2b".into() });
        c.history.push(Turn::User("two, edited".into()));
        c.history.push(Turn::Assistant { text: "a2b".into(), calls: vec![] });
        c.branches = c.snapshot_branches();
        assert_eq!(c.take_counts(), vec![(1, 1, 2)]);
        assert!(c.switch_version(1, -1));
        assert_eq!(c.entries[2].text, "two");
        assert_eq!(c.entries[3].text, "a2");
        assert_eq!(c.history.len(), 4);
        assert!(c.switch_version(1, 1));
        assert_eq!(c.entries[2].text, "two, edited");
        assert!(!c.switch_version(1, 1), "no take after the last");
    }
}
