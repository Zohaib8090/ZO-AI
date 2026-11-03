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
        delete("a");
        assert_eq!(load().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_DATA_HOME");
    }
}
