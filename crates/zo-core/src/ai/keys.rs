//! API keys live in the system keyring (KDE Wallet / any Secret Service) through `secret-tool`, never in a plain file.
//! `ZOHARA_AGENT_KEY_<PROVIDER>` in the environment wins, for scripts and tests.
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

/// The environment names a provider key is read from, the preferred one first: `ZOHARA_AGENT_KEY_<ID>`, then the names before the
/// renames (`ZOHARA_CHAT_KEY_<ID>`, `ZOHARA_AI_KEY_<ID>`, `ZO_AI_KEY_<ID>`, `ZOAI_KEY_<ID>`).
fn env_names(id: &str) -> Vec<String> {
    let id = id.to_uppercase().replace('-', "_");
    crate::legacy::ENV_PREFIXES.iter().map(|p| format!("{p}KEY_{id}")).chain([format!("ZOAI_KEY_{id}")]).collect()
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

/// On a server there is no keyring: with `ZOHARA_AGENT_KEYS_FILE` set, keys are kept in that file instead (the server keeps it private
/// and stores it encrypted).
fn file() -> Option<std::path::PathBuf> {
    crate::legacy::env_os("KEYS_FILE").map(std::path::PathBuf::from)
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
    for name in env_names(id) {
        if let Ok(k) = std::env::var(name) {
            if !k.trim().is_empty() {
                return Some(k.trim().to_string());
            }
        }
    }
    if let Some(p) = file() {
        return file_load(&p).get(id).and_then(|v| v.as_str()).filter(|k| !k.is_empty()).map(str::to_string);
    }
    if let Some(key) = keyring_lookup(crate::legacy::NAME, id) {
        return Some(key);
    }
    // a key stored before a rename sits under an older service name (Zohara Chat, then ZO-AI): use it and copy it to the new one (the old entry stays)
    let key = crate::legacy::OLD_NAMES.iter().find_map(|old| keyring_lookup(old, id))?;
    let _ = keyring_store(crate::legacy::NAME, id, &key);
    Some(key)
}

fn keyring_lookup(service: &str, id: &str) -> Option<String> {
    let child = Command::new(secret_tool()).args(["lookup", "service", service, "provider", id]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let out = finish(child, KEYRING_WAIT).ok()?;
    let key = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !key.is_empty()).then_some(key)
}

fn keyring_store(service: &str, id: &str, key: &str) -> Result<(), String> {
    let label = format!("Zohara Agent key for {id}");
    let mut child = Command::new(secret_tool())
        .args(["store", "--label", &label, "service", service, "provider", id])
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
    keyring_store(crate::legacy::NAME, id, key)
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
    // forgetting a key forgets it under the older service names too, or it would come back from there
    for service in std::iter::once(crate::legacy::NAME).chain(crate::legacy::OLD_NAMES) {
        if let Ok(child) = Command::new(secret_tool()).args(["clear", "service", service, "provider", id]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            let _ = finish(child, KEYRING_WAIT);
        }
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A fake `secret-tool` that keeps entries as files named <service>__<provider> in $FAKE_KEYRING.
    fn fake_tool(dir: &std::path::Path) -> std::path::PathBuf {
        let p = dir.join("secret-tool");
        std::fs::write(
            &p,
            r#"#!/bin/sh
cmd="$1"; shift
[ "$cmd" = "--version" ] && exit 0
[ "$cmd" = "store" ] && shift 2
service="$2"; provider="$4"
f="$FAKE_KEYRING/${service}__${provider}"
case "$cmd" in
  lookup) [ -f "$f" ] && cat "$f" || exit 1 ;;
  store) cat > "$f" ;;
  clear) rm -f "$f" ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn sandbox(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("zo-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let ring = d.join("ring");
        std::fs::create_dir_all(&ring).unwrap();
        std::env::set_var("ZO_TEST_SECRET_TOOL", fake_tool(&d));
        std::env::set_var("FAKE_KEYRING", &ring);
        for v in ["ZOHARA_AGENT_KEYS_FILE", "ZOHARA_CHAT_KEYS_FILE", "ZO_AI_KEYS_FILE"] {
            std::env::remove_var(v);
        }
        (d, ring)
    }

    fn done(d: &std::path::Path) {
        std::env::remove_var("ZO_TEST_SECRET_TOOL");
        std::env::remove_var("FAKE_KEYRING");
        let _ = std::fs::remove_dir_all(d);
    }

    /// A key stored before a rename (service "zohara-chat" or "zo-ai") is found, copied to the new service once, and left in the old one.
    #[test]
    fn a_key_in_the_old_keyring_service_is_copied_to_the_new_one() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (d, ring) = sandbox("migrate");
        for n in env_names("renametest") {
            std::env::remove_var(n);
        }
        std::fs::write(ring.join("zo-ai__renametest"), "sk-old").unwrap();
        assert_eq!(get("renametest").as_deref(), Some("sk-old"));
        assert_eq!(std::fs::read_to_string(ring.join("zohara-agent__renametest")).unwrap(), "sk-old", "copied to the new service");
        assert!(ring.join("zo-ai__renametest").exists(), "the old entry is kept");
        // after the copy the new entry is the one that counts
        std::fs::write(ring.join("zohara-agent__renametest"), "sk-new").unwrap();
        assert_eq!(get("renametest").as_deref(), Some("sk-new"));
        // new keys go to the new service only
        set_inner("renametest", "sk-set").unwrap();
        assert_eq!(std::fs::read_to_string(ring.join("zohara-agent__renametest")).unwrap(), "sk-set");
        assert_eq!(std::fs::read_to_string(ring.join("zo-ai__renametest")).unwrap(), "sk-old");
        // forgetting removes all of them, or an old one would bring the key back
        clear_inner("renametest");
        assert!(get("renametest").is_none());
        assert!(!ring.join("zo-ai__renametest").exists() && !ring.join("zohara-agent__renametest").exists());
        done(&d);
    }

    /// An install at the Zohara Chat stage (service "zohara-chat", which can sit next to an older "zo-ai" entry): the Zohara Chat key wins.
    #[test]
    fn a_key_in_the_zohara_chat_service_is_copied_and_wins_over_zo_ai() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (d, ring) = sandbox("chat");
        for n in env_names("chatstage") {
            std::env::remove_var(n);
        }
        std::fs::write(ring.join("zo-ai__chatstage"), "sk-ancient").unwrap();
        std::fs::write(ring.join("zohara-chat__chatstage"), "sk-chat").unwrap();
        assert_eq!(get("chatstage").as_deref(), Some("sk-chat"));
        assert_eq!(std::fs::read_to_string(ring.join("zohara-agent__chatstage")).unwrap(), "sk-chat", "copied to the new service");
        assert!(ring.join("zohara-chat__chatstage").exists() && ring.join("zo-ai__chatstage").exists(), "both old entries are kept");
        // forgetting clears all three services
        clear_inner("chatstage");
        assert!(get("chatstage").is_none());
        assert!(!ring.join("zohara-chat__chatstage").exists() && !ring.join("zo-ai__chatstage").exists() && !ring.join("zohara-agent__chatstage").exists());
        done(&d);
    }

    #[test]
    fn the_old_environment_names_still_work_and_the_new_one_wins() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let names = env_names("renameenv");
        assert_eq!(names[0], "ZOHARA_AGENT_KEY_RENAMEENV");
        assert!(names.contains(&"ZOHARA_CHAT_KEY_RENAMEENV".to_string()) && names.contains(&"ZOAI_KEY_RENAMEENV".to_string()));
        for n in &names {
            std::env::remove_var(n);
        }
        // from the oldest name up to the newest: each newer one wins
        for (n, v) in names.iter().rev().zip(["sk-zoai", "sk-zo-ai", "sk-zohara-ai", "sk-chat", "sk-agent"]) {
            std::env::set_var(n, v);
            assert_eq!(get("renameenv").as_deref(), Some(v), "{n}");
        }
        for n in &names {
            std::env::remove_var(n);
        }
    }

    #[test]
    fn the_old_keys_file_variables_still_work() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let vars = ["ZOHARA_AGENT_KEYS_FILE", "ZOHARA_CHAT_KEYS_FILE", "ZO_AI_KEYS_FILE"];
        for v in vars {
            std::env::remove_var(v);
        }
        for v in vars.iter().rev() {
            std::env::set_var(v, format!("/tmp/{v}.json"));
            assert_eq!(file().as_deref(), Some(std::path::Path::new(&format!("/tmp/{v}.json"))));
        }
        for v in vars {
            std::env::remove_var(v);
        }
    }
}
