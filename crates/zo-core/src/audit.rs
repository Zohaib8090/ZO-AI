//! A plain log of what the AI did, one JSON object per line: ~/.local/share/zo-ai/audit.jsonl

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()))
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("zo-ai")
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("zo-ai")
}

pub fn log(tool: &str, args: &serde_json::Value, outcome: &str) {
    let mut shown = args.to_string();
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
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("audit.jsonl")) {
        let _ = writeln!(f, "{line}");
    }
}
