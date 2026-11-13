//! A plain log of what the AI did, one JSON object per line: ~/.local/share/zohara-chat/audit.jsonl (before the rename: ~/.local/share/zo-ai/)

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()))
}

/// The folder for settings. A tenant has its own; a multi-tenant server never falls back to the default one.
pub fn config_dir() -> PathBuf {
    if let Some(t) = crate::tenant::current() {
        return t.config;
    }
    if crate::tenant::multi_tenant() {
        return crate::tenant::unusable().join("config");
    }
    let base = config_base();
    crate::legacy::migrate_dir(&base.join(crate::legacy::OLD_NAME), &base.join(crate::legacy::NEW_NAME))
}

fn config_base() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
}

fn data_base() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"))
}

/// The settings folder of the old name (ZO-AI). Still on disk after the move to `config_dir()`; the AI must not read it either.
pub fn old_config_dir() -> PathBuf {
    config_base().join(crate::legacy::OLD_NAME)
}

pub fn data_dir() -> PathBuf {
    if let Some(t) = crate::tenant::current() {
        return t.data;
    }
    if crate::tenant::multi_tenant() {
        return crate::tenant::unusable().join("data");
    }
    let base = data_base();
    crate::legacy::migrate_dir(&base.join(crate::legacy::OLD_NAME), &base.join(crate::legacy::NEW_NAME))
}

pub fn log(tool: &str, args: &serde_json::Value, outcome: &str) {
    // a restricted person's words are never logged, only which tool ran
    let restricted = crate::tenant::is_restricted();
    let mut shown = if restricted { "(not logged)".to_string() } else { args.to_string() };
    if shown.len() > 400 {
        shown.truncate(400);
        shown.push('…');
    }
    let line = serde_json::json!({
        "time": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "tool": tool, "args": shown, "outcome": outcome,
    });
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    // the log of a restricted person stays small
    if restricted && std::fs::metadata(dir.join("audit.jsonl")).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        return;
    }
    if !crate::tenant::fits(&dir.join("audit.jsonl"), line.to_string().len() + 1 + std::fs::metadata(dir.join("audit.jsonl")).map(|m| m.len() as usize).unwrap_or(0)) {
        return;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("audit.jsonl")) {
        let _ = writeln!(f, "{line}");
    }
}

#[cfg(test)]
mod rename_tests {
    /// The settings and the chats of the version before the rename (folders called zo-ai) are copied to the new folders once;
    /// the old folders stay.
    #[test]
    fn config_and_data_folders_move_to_the_new_name_once() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!("zo-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (conf, data) = (d.join("conf"), d.join("data"));
        std::fs::create_dir_all(conf.join("zo-ai")).unwrap();
        std::fs::create_dir_all(data.join("zo-ai")).unwrap();
        std::fs::write(conf.join("zo-ai/sync.json"), "{\"enabled\":true}").unwrap();
        std::fs::write(data.join("zo-ai/chats.json"), "[1]").unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &conf);
        std::env::set_var("XDG_DATA_HOME", &data);
        assert_eq!(super::config_dir(), conf.join("zohara-chat"));
        assert_eq!(super::data_dir(), data.join("zohara-chat"));
        assert_eq!(super::old_config_dir(), conf.join("zo-ai"));
        assert_eq!(std::fs::read_to_string(conf.join("zohara-chat/sync.json")).unwrap(), "{\"enabled\":true}");
        assert_eq!(std::fs::read_to_string(data.join("zohara-chat/chats.json")).unwrap(), "[1]");
        assert!(conf.join("zo-ai/sync.json").exists() && data.join("zo-ai/chats.json").exists(), "the old folders are kept");
        // later changes live in the new folder and are not overwritten by a second move
        std::fs::write(conf.join("zohara-chat/sync.json"), "{}").unwrap();
        let _ = super::config_dir();
        assert_eq!(std::fs::read_to_string(conf.join("zohara-chat/sync.json")).unwrap(), "{}");
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        let _ = std::fs::remove_dir_all(&d);
    }
}
