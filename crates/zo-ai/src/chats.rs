//! Saved chats: what the sidebar lists under Recents, and what opens again when one is clicked.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use zo_core::ai::wire::Turn;

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
pub fn upsert(chat: Chat) {
    let mut all = load();
    all.retain(|c| c.id != chat.id);
    all.insert(0, chat);
    all.truncate(KEEP);
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
