//! API keys live in the system keyring (KDE Wallet / any Secret Service) through `secret-tool`, never in a plain file.
//! `ZOAI_KEY_<PROVIDER>` in the environment wins, for scripts and tests.
//!
//! A tenant (see `tenant`) has no keyring and no environment keys: its keys are in `<config>/keys.enc`, encrypted with AES-256-GCM
//! (key = sha256("zo-keys" || ZO_SERVER_SECRET || tenant id); the file is a 12 byte nonce followed by the ciphertext). There is no
//! shared key for everyone: each person brings their own.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::process::{Command, Stdio};

/// The name of a tenant's key file inside its config folder.
pub const ENC_FILE: &str = "keys.enc";

fn env_name(id: &str) -> String {
    format!("ZOAI_KEY_{}", id.to_uppercase().replace('-', "_"))
}

type KeyMap = serde_json::Map<String, serde_json::Value>;

fn cipher(secret: &str, tenant_id: &str) -> Aes256Gcm {
    let mut h = Sha256::new();
    h.update(b"zo-keys");
    h.update(secret.as_bytes());
    h.update(tenant_id.as_bytes());
    Aes256Gcm::new_from_slice(&h.finalize()).expect("32 bytes")
}

/// The bytes of a key file: nonce, then the encrypted JSON of the keys.
pub fn encrypt_map(secret: &str, tenant_id: &str, m: &KeyMap) -> Result<Vec<u8>, String> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher(secret, tenant_id).encrypt(&nonce, serde_json::Value::Object(m.clone()).to_string().as_bytes()).map_err(|_| "The keys could not be encrypted.".to_string())?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(out)
}

/// Opens a key file. Fails for another secret, another tenant or a damaged file.
pub fn decrypt_map(secret: &str, tenant_id: &str, raw: &[u8]) -> Result<KeyMap, String> {
    if raw.len() < 12 + 16 {
        return Err("The key file is damaged.".into());
    }
    let plain = cipher(secret, tenant_id).decrypt(Nonce::from_slice(&raw[..12]), &raw[12..]).map_err(|_| "The saved keys cannot be opened (wrong server secret or another account).".to_string())?;
    serde_json::from_slice(&plain).map_err(|_| "The key file is damaged.".to_string())
}

/// The server's secret, kept here once the server has read it at start and taken it out of its own environment (so no program it
/// starts, and no `/proc/<pid>/environ`, can show it). Tests and older set-ups still use the variable.
static SERVER_SECRET: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Remembers the secret for this process (the server calls this before it removes `ZO_SERVER_SECRET` from its environment).
pub fn set_server_secret(secret: &str) {
    *SERVER_SECRET.lock().unwrap_or_else(|e| e.into_inner()) = Some(secret.to_string()).filter(|s| !s.is_empty());
}

fn secret() -> Result<String, String> {
    if let Some(s) = SERVER_SECRET.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Ok(s);
    }
    std::env::var("ZO_SERVER_SECRET").ok().filter(|s| !s.is_empty()).ok_or_else(|| "The server has no secret set, so keys cannot be stored safely.".to_string())
}

/// The file and owner of the current tenant's keys; `Some(None)` is a multi-tenant server without a tenant: nothing is readable.
fn tenant_keys() -> Option<Option<(std::path::PathBuf, String)>> {
    match crate::tenant::current() {
        Some(t) => Some(Some((t.config.join(ENC_FILE), t.id))),
        None if crate::tenant::multi_tenant() => Some(None),
        None => None,
    }
}

fn enc_load(p: &std::path::Path, id: &str) -> Result<KeyMap, String> {
    match std::fs::read(p) {
        Ok(raw) => decrypt_map(&secret()?, id, &raw),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(KeyMap::new()),
        Err(e) => Err(e.to_string()),
    }
}

fn enc_save(p: &std::path::Path, id: &str, m: &KeyMap) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let bytes = encrypt_map(&secret()?, id, m)?;
    if !crate::tenant::fits(p, bytes.len()) {
        return Err(crate::tenant::FULL_MSG.into());
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("enc.tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(&bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
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

/// How long the keyring tool may take (a locked wallet can wait for a person to type a password; nothing waits for that forever).
const KEYRING_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// Waits for a child with a deadline; kills it when the deadline passes. Output is read after it has ended (these are tiny).
fn finish(mut child: std::process::Child, wait: std::time::Duration) -> Result<std::process::Output, String> {
    let end = std::time::Instant::now() + wait;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) if std::time::Instant::now() < end => std::thread::sleep(std::time::Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The keyring did not answer in time (is it locked?).".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The keyring program. Only tests may point it somewhere else (to simulate a computer without one): changing PATH for that
/// would break every other test that starts a program at the same moment.
fn secret_tool() -> std::ffi::OsString {
    #[cfg(test)]
    if let Some(v) = std::env::var_os("ZO_TEST_SECRET_TOOL").filter(|v| !v.is_empty()) {
        return v;
    }
    "secret-tool".into()
}

/// Saves a key. A provider's key also gets a time for the sync (see `sync`).
pub fn set(id: &str, key: &str) -> Result<(), String> {
    set_inner(id, key)?;
    crate::sync::key_touched(id, false);
    Ok(())
}

/// Forgets a key. A provider's key also leaves a tombstone for the sync.
pub fn clear(id: &str) {
    clear_inner(id);
    crate::sync::key_touched(id, true);
}

pub fn available() -> bool {
    if let Some(t) = tenant_keys() {
        return t.is_some() && secret().is_ok();
    }
    if file().is_some() {
        return true;
    }
    Command::new(secret_tool()).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

pub fn get(id: &str) -> Option<String> {
    // a tenant's keys are only its own: no environment key, no shared file, no keyring
    if let Some(t) = tenant_keys() {
        let (p, tid) = t?;
        return enc_load(&p, &tid).ok()?.get(id).and_then(|v| v.as_str()).filter(|k| !k.is_empty()).map(str::to_string);
    }
    if let Ok(k) = std::env::var(env_name(id)) {
        if !k.trim().is_empty() {
            return Some(k.trim().to_string());
        }
    }
    if let Some(p) = file() {
        return file_load(&p).get(id).and_then(|v| v.as_str()).filter(|k| !k.is_empty()).map(str::to_string);
    }
    let child = Command::new(secret_tool()).args(["lookup", "service", "zo-ai", "provider", id]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let out = finish(child, KEYRING_WAIT).ok()?;
    let key = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !key.is_empty()).then_some(key)
}

fn set_inner(id: &str, key: &str) -> Result<(), String> {
    if let Some(t) = tenant_keys() {
        let (p, tid) = t.ok_or("No account is signed in.")?;
        let mut m = enc_load(&p, &tid)?; // an unreadable file is never overwritten
        m.insert(id.to_string(), serde_json::Value::String(key.trim().to_string()));
        return enc_save(&p, &tid, &m);
    }
    if let Some(p) = file() {
        let mut m = file_load(&p);
        m.insert(id.to_string(), serde_json::Value::String(key.trim().to_string()));
        return file_save(&p, &m);
    }
    let label = format!("ZO-AI key for {id}");
    let mut child = Command::new(secret_tool())
        .args(["store", "--label", &label, "service", "zo-ai", "provider", id])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "The keyring tool (secret-tool, from libsecret) is not installed, so the key cannot be stored safely.".to_string())?;
    child.stdin.take().ok_or("no stdin")?.write_all(key.trim().as_bytes()).map_err(|e| e.to_string())?;
    let out = finish(child, KEYRING_WAIT)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("The keyring refused the key: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn clear_inner(id: &str) {
    if let Some(t) = tenant_keys() {
        if let Some((p, tid)) = t {
            if let Ok(mut m) = enc_load(&p, &tid) {
                m.remove(id);
                let _ = enc_save(&p, &tid, &m);
            }
        }
        return;
    }
    if let Some(p) = file() {
        let mut m = file_load(&p);
        m.remove(id);
        let _ = file_save(&p, &m);
        return;
    }
    if let Ok(child) = Command::new(secret_tool()).args(["clear", "service", "zo-ai", "provider", id]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        let _ = finish(child, KEYRING_WAIT);
    }
}
