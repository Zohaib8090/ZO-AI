//! A plain log of what the AI did, one JSON object per line: ~/.local/share/zohara-agent/audit.jsonl (before the renames: ~/.local/share/zohara-chat/, ~/.local/share/zo-ai/)

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
    crate::legacy::migrate_under(&config_base())
}

fn config_base() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
}

fn data_base() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"))
}

/// The settings folders of the older names (Zohara Chat, ZO-AI). Still on disk after the move to `config_dir()`; the AI must not read them either.
pub fn old_config_dirs() -> Vec<PathBuf> {
    crate::legacy::old_under(&config_base())
}

pub fn data_dir() -> PathBuf {
    if let Some(t) = crate::tenant::current() {
        return t.data;
    }
    if crate::tenant::multi_tenant() {
        return crate::tenant::unusable().join("data");
    }
    crate::legacy::migrate_under(&data_base())
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
    fn sandbox(name: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("zo-audit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (conf, data) = (d.join("conf"), d.join("data"));
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &conf);
        std::env::set_var("XDG_DATA_HOME", &data);
        (d, conf, data)
    }

    fn done(d: &std::path::Path) {
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        let _ = std::fs::remove_dir_all(d);
    }

    /// An install at the ZO-AI stage (folders called zo-ai): the settings and the chats are copied to the new folders once; the old ones stay.
    #[test]
    fn config_and_data_folders_move_from_zo_ai_once() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (d, conf, data) = sandbox("zoai");
        std::fs::create_dir_all(conf.join("zo-ai")).unwrap();
        std::fs::create_dir_all(data.join("zo-ai")).unwrap();
        std::fs::write(conf.join("zo-ai/sync.json"), "{\"enabled\":true}").unwrap();
        std::fs::write(data.join("zo-ai/chats.json"), "[1]").unwrap();
        assert_eq!(super::config_dir(), conf.join("zohara-agent"));
        assert_eq!(super::data_dir(), data.join("zohara-agent"));
        assert_eq!(super::old_config_dirs(), vec![conf.join("zohara-chat"), conf.join("zo-ai")]);
        assert_eq!(std::fs::read_to_string(conf.join("zohara-agent/sync.json")).unwrap(), "{\"enabled\":true}");
        assert_eq!(std::fs::read_to_string(data.join("zohara-agent/chats.json")).unwrap(), "[1]");
        assert!(conf.join("zo-ai/sync.json").exists() && data.join("zo-ai/chats.json").exists(), "the old folders are kept");
        // later changes live in the new folder and are not overwritten by a second move
        std::fs::write(conf.join("zohara-agent/sync.json"), "{}").unwrap();
        let _ = super::config_dir();
        assert_eq!(std::fs::read_to_string(conf.join("zohara-agent/sync.json")).unwrap(), "{}");
        done(&d);
    }

    /// An install at the Zohara Chat stage (folders called zohara-chat, which may sit next to an older zo-ai): the Zohara Chat folders win.
    #[test]
    fn config_and_data_folders_move_from_zohara_chat_once() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (d, conf, data) = sandbox("chat");
        for b in [&conf, &data] {
            std::fs::create_dir_all(b.join("zo-ai")).unwrap();
            std::fs::create_dir_all(b.join("zohara-chat")).unwrap();
        }
        std::fs::write(conf.join("zo-ai/sync.json"), "ancient").unwrap();
        std::fs::write(conf.join("zohara-chat/sync.json"), "chat-stage").unwrap();
        std::fs::write(data.join("zohara-chat/chats.json"), "[2]").unwrap();
        assert_eq!(super::config_dir(), conf.join("zohara-agent"));
        assert_eq!(super::data_dir(), data.join("zohara-agent"));
        assert_eq!(std::fs::read_to_string(conf.join("zohara-agent/sync.json")).unwrap(), "chat-stage");
        assert_eq!(std::fs::read_to_string(data.join("zohara-agent/chats.json")).unwrap(), "[2]");
        assert!(conf.join("zohara-chat/sync.json").exists() && conf.join("zo-ai/sync.json").exists(), "both old folders are kept");
        done(&d);
    }

    /// A fresh install has nothing to move and gets no old folders made.
    #[test]
    fn a_fresh_install_makes_no_old_folders() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (d, conf, _data) = sandbox("fresh");
        assert_eq!(super::config_dir(), conf.join("zohara-agent"));
        assert!(!conf.join("zohara-chat").exists() && !conf.join("zo-ai").exists());
        done(&d);
    }
}
