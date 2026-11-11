//! API keys live in the system keyring (KDE Wallet / any Secret Service) through `secret-tool`, never in a plain file.
//! `ZOAI_KEY_<PROVIDER>` in the environment wins, for scripts and tests.

use std::io::Write;
use std::process::{Command, Stdio};

fn env_name(id: &str) -> String {
    format!("ZOAI_KEY_{}", id.to_uppercase().replace('-', "_"))
}

/// On a server there is no keyring: with `ZO_AI_KEYS_FILE` set, keys are kept in that file instead (the server keeps it private
/// and stores it encrypted).
fn file() -> Option<std::path::PathBuf> {
    std::env::var_os("ZO_AI_KEYS_FILE").filter(|p| !p.is_empty()).map(std::path::PathBuf::from)
}

fn file_load(p: &std::path::Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn file_save(p: &std::path::Path, m: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p).map_err(|e| e.to_string())?;
    f.write_all(serde_json::Value::Object(m.clone()).to_string().as_bytes()).map_err(|e| e.to_string())
}

pub fn available() -> bool {
    if file().is_some() {
        return true;
    }
    Command::new("secret-tool").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

pub fn get(id: &str) -> Option<String> {
    if let Ok(k) = std::env::var(env_name(id)) {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    if let Some(p) = file() {
        return file_load(&p).get(id).and_then(|v| v.as_str()).filter(|k| !k.is_empty()).map(str::to_string);
    }
    let out = Command::new("secret-tool").args(["lookup", "service", "zo-ai", "provider", id]).stderr(Stdio::null()).output().ok()?;
    let key = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !key.is_empty()).then_some(key)
}

pub fn set(id: &str, key: &str) -> Result<(), String> {
    if let Some(p) = file() {
        let mut m = file_load(&p);
        m.insert(id.to_string(), serde_json::Value::String(key.trim().to_string()));
        return file_save(&p, &m);
    }
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
    if let Some(p) = file() {
        let mut m = file_load(&p);
        m.remove(id);
        let _ = file_save(&p, &m);
        return;
    }
    let _ = Command::new("secret-tool").args(["clear", "service", "zo-ai", "provider", id]).stderr(Stdio::null()).status();
}
