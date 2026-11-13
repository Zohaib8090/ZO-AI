//! Artifacts: what Zohara Agent has made for you. Files it wrote, screenshots it took, and code from its replies.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Artifact {
    pub id: String,
    /// file, screenshot or code.
    pub kind: String,
    pub title: String,
    /// Where it is on disk (files and screenshots).
    #[serde(default)]
    pub path: Option<String>,
    /// The text itself (code).
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub chat: Option<String>,
    pub created: u64,
}

const KEEP: usize = 200;
const MAX_TEXT: usize = 100_000;

fn file() -> PathBuf {
    crate::audit::data_dir().join("artifacts.json")
}

/// Newest first.
pub fn load() -> Vec<Artifact> {
    let mut v: Vec<Artifact> = std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.sort_by(|a, b| b.created.cmp(&a.created));
    v
}

fn write(all: &[Artifact]) {
    let p = file();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("json.tmp");
    if let Ok(s) = serde_json::to_string(all) {
        if !crate::tenant::fits(&p, s.len()) {
            return;
        }
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

fn now() -> u64 {
    super::schedules::now_millis()
}

fn push(mut a: Artifact) {
    let mut all = load();
    // the same file written again is one artifact, moved to the top
    if let Some(path) = &a.path {
        all.retain(|x| x.path.as_ref() != Some(path));
    }
    // the same code shown again in the same chat is not repeated
    if let (Some(t), Some(c)) = (&a.text, &a.chat) {
        all.retain(|x| !(x.text.as_ref() == Some(t) && x.chat.as_ref() == Some(c)));
    }
    a.id = format!("a{}{}", a.created, all.len());
    all.insert(0, a);
    all.truncate(KEEP);
    write(&all);
}

pub fn add_file(path: &str, kind: &str) {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{rest}", std::env::var("HOME").unwrap_or_default())
    } else {
        path.to_string()
    };
    let title = std::path::Path::new(&expanded).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| expanded.clone());
    push(Artifact { kind: kind.into(), title, path: Some(expanded), created: now(), ..Default::default() });
}

pub fn add_code(lang: &str, text: &str, chat: Option<String>) {
    if text.lines().count() < 3 {
        return; // one-liners are not worth keeping
    }
    let title = if lang.is_empty() { "code".to_string() } else { lang.to_string() };
    push(Artifact { kind: "code".into(), title, text: Some(text.chars().take(MAX_TEXT).collect()), chat, created: now(), ..Default::default() });
}

pub fn delete(id: &str) {
    let mut all = load();
    all.retain(|a| a.id != id);
    write(&all);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_screenshots_and_code_are_collected_without_repeats() {
        let _g = super::super::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-artifacts-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", &dir);

        add_file("/tmp/notes.md", "file");
        add_file("/tmp/notes.md", "file");
        add_file("/home/x/screen-1.png", "screenshot");
        add_code("rust", "fn main() {\n    println!(\"hi\");\n}", Some("c1".into()));
        add_code("rust", "fn main() {\n    println!(\"hi\");\n}", Some("c1".into()));
        add_code("sh", "ls", Some("c1".into()));
        let all = load();
        assert_eq!(all.len(), 3, "a file written twice is one, repeated code is one, a one-liner is skipped");
        assert_eq!(all.iter().filter(|a| a.kind == "file").count(), 1);
        assert_eq!(all.iter().find(|a| a.kind == "screenshot").unwrap().title, "screen-1.png");
        let id = all[0].id.clone();
        delete(&id);
        assert_eq!(load().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_DATA_HOME");
    }
}
