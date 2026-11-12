//! A plain log of what the AI did, one JSON object per line: ~/.local/share/zo-ai/audit.jsonl

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
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("zo-ai")
}

pub fn data_dir() -> PathBuf {
    if let Some(t) = crate::tenant::current() {
        return t.data;
    }
    if crate::tenant::multi_tenant() {
        return crate::tenant::unusable().join("data");
    }
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("zo-ai")
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
