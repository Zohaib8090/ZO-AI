//! API keys live in the system keyring (KDE Wallet / any Secret Service) through `secret-tool`, never in a plain file.
//! `ZOAI_KEY_<PROVIDER>` in the environment wins, for scripts and tests.

use std::io::Write;
use std::process::{Command, Stdio};

fn env_name(id: &str) -> String {
    format!("ZOAI_KEY_{}", id.to_uppercase().replace('-', "_"))
}

pub fn available() -> bool {
    Command::new("secret-tool").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

pub fn get(id: &str) -> Option<String> {
    if let Ok(k) = std::env::var(env_name(id)) {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    let out = Command::new("secret-tool").args(["lookup", "service", "zo-ai", "provider", id]).stderr(Stdio::null()).output().ok()?;
    let key = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !key.is_empty()).then_some(key)
}

pub fn set(id: &str, key: &str) -> Result<(), String> {
    let label = format!("ZO-AI key for {id}");
    let mut child = Command::new("secret-tool")
        .args(["store", "--label", &label, "service", "zo-ai", "provider", id])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "The keyring tool (secret-tool, from libsecret) is not installed, so the key cannot be stored safely.".to_string())?;
    child.stdin.take().ok_or("no stdin")?.write_all(key.trim().as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("The keyring refused the key: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

pub fn clear(id: &str) {
    let _ = Command::new("secret-tool").args(["clear", "service", "zo-ai", "provider", id]).stderr(Stdio::null()).status();
}
