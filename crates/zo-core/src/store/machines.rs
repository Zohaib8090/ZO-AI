//! Machines Zohara Agent may work on over SSH (a cloud VM, a server at home). The list is in `~/.config/zohara-agent/machines.json`; the private
//! key of each machine is a secret and lives in the key store (`ai::keys`, id `ssh-<name>`), never in that file.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Machine {
    pub name: String,
    pub host: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub note: String,
}

fn default_port() -> u16 {
    22
}

fn path() -> PathBuf {
    crate::audit::config_dir().join("machines.json")
}

pub fn key_id(name: &str) -> String {
    format!("ssh-{name}")
}

/// A word that can safely end up inside an ssh command line: letters, digits, `.`, `_` and `-`, not starting with `-` or `.`.
/// So it can never be read as an ssh option, and holds no space, quote or shell character.
fn word(what: &str, v: &str, max: usize) -> Result<(), String> {
    if v.is_empty() {
        return Err(format!("The {what} is empty."));
    }
    if v.len() > max {
        return Err(format!("The {what} is too long (at most {max} characters)."));
    }
    if v.starts_with('-') || v.starts_with('.') {
        return Err(format!("The {what} must not start with \"{}\".", &v[..1]));
    }
    if !v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Err(format!("The {what} may only have letters, digits, \".\", \"_\" and \"-\"."));
    }
    Ok(())
}

impl Machine {
    pub fn validate(&self) -> Result<(), String> {
        word("name", &self.name, 40)?;
        word("host", &self.host, 253)?;
        word("user", &self.user, 32)?;
        if self.port == 0 {
            return Err("The port must be from 1 to 65535.".into());
        }
        if self.note.chars().count() > 300 || self.note.contains('\0') {
            return Err("The note is too long (at most 300 characters).".into());
        }
        Ok(())
    }

    pub fn has_key(&self) -> bool {
        crate::ai::keys::get(&key_id(&self.name)).is_some()
    }
}

/// Reads a port typed by a person ("22", "2222").
pub fn parse_port(s: &str) -> Result<u16, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(22);
    }
    match s.parse::<u32>() {
        Ok(p) if (1..=65535).contains(&p) => Ok(p as u16),
        _ => Err("The port must be a number from 1 to 65535.".into()),
    }
}

/// A private key as pasted: checked to look like one, line endings made plain, one newline at the end (ssh needs it).
pub fn clean_key(text: &str) -> Result<String, String> {
    let t = text.replace("\r\n", "\n").replace('\r', "\n");
    let t = t.trim();
    if t.len() > 16_000 {
        return Err("That is too long to be a private key.".into());
    }
    if !(t.starts_with("-----BEGIN ") && t.contains("PRIVATE KEY-----") && t.contains("-----END ")) {
        return Err("That does not look like a private key (it should start with -----BEGIN ... PRIVATE KEY-----).".into());
    }
    if t.contains("ENCRYPTED") {
        return Err("This key is protected by a passphrase. Zohara Agent cannot type one: make a key without a passphrase for it.".into());
    }
    Ok(format!("{t}\n"))
}

pub fn load() -> Vec<Machine> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(list: &[Machine]) -> Result<(), String> {
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

pub fn get(name: &str) -> Option<Machine> {
    load().into_iter().find(|m| m.name == name)
}

/// Adds a machine, or changes the one with the same name. `key` is the pasted private key; it may be left out when the machine
/// already has one.
pub fn add(m: Machine, key: Option<&str>) -> Result<(), String> {
    m.validate()?;
    let id = key_id(&m.name);
    match key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => crate::ai::keys::set(&id, &clean_key(k)?)?,
        None if crate::ai::keys::get(&id).is_none() => return Err("Paste the private key that this machine accepts.".into()),
        None => {}
    }
    let mut list = load();
    list.retain(|x| x.name != m.name);
    list.push(m);
    list.sort_by(|a, b| a.name.cmp(&b.name));
    save(&list)
}

pub fn remove(name: &str) -> Result<(), String> {
    let mut list = load();
    let before = list.len();
    list.retain(|m| m.name != name);
    if list.len() == before {
        return Err("There is no such machine.".into());
    }
    crate::ai::keys::clear(&key_id(name));
    save(&list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, host: &str, user: &str, port: u16) -> Machine {
        Machine { name: name.into(), host: host.into(), user: user.into(), port, note: String::new() }
    }

    #[test]
    fn good_machines_pass() {
        assert!(m("cloud-1", "203.0.113.7", "ubuntu", 22).validate().is_ok());
        assert!(m("home.server", "vm_1.example.com", "zo", 2222).validate().is_ok());
    }

    #[test]
    fn nasty_inputs_are_refused() {
        let long = "a".repeat(300);
        for bad in ["", "-oProxyCommand=x", "a b", "a;b", "a'b", "a\"b", "a\nb", "a$(x)", "a`b`", "-", ".hidden", "a/b", "ä", "a@b", "[::1]", &long] {
            assert!(m(bad, "h", "u", 22).validate().is_err(), "name {bad:?}");
            assert!(m("n", bad, "u", 22).validate().is_err(), "host {bad:?}");
            assert!(m("n", "h", bad, 22).validate().is_err(), "user {bad:?}");
        }
        assert!(m("n", "h", "u", 0).validate().is_err());
    }

    #[test]
    fn ports() {
        assert_eq!(parse_port("").unwrap(), 22);
        assert_eq!(parse_port(" 2222 ").unwrap(), 2222);
        assert_eq!(parse_port("65535").unwrap(), 65535);
        for bad in ["0", "65536", "-1", "22; ls", "abc", "99999999999"] {
            assert!(parse_port(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn keys_are_checked_and_tidied() {
        let k = "-----BEGIN OPENSSH PRIVATE KEY-----\r\nabc\r\n-----END OPENSSH PRIVATE KEY-----\r\n\r\n";
        assert_eq!(clean_key(k).unwrap(), "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----\n");
        assert!(clean_key("hello").is_err());
        assert!(clean_key("-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n-----END RSA PRIVATE KEY-----").is_err());
        assert!(clean_key(&"x".repeat(20_000)).is_err());
    }

    #[test]
    fn add_list_remove_never_writes_the_key_to_the_list() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-machines-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        std::env::set_var("ZOHARA_AGENT_KEYS_FILE", dir.join("keys.json"));
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\nSECRETSECRET\n-----END OPENSSH PRIVATE KEY-----";
        assert!(add(m("a", "h", "u", 22), None).is_err(), "no key, no machine");
        assert!(add(m("a", "h", "u", 22), Some("nope")).is_err());
        add(m("a", "h", "u", 22), Some(key)).unwrap();
        add(m("b", "h2", "u", 2222), Some(key)).unwrap();
        let list = load();
        assert_eq!(list.len(), 2);
        assert!(get("a").unwrap().has_key());
        let raw = std::fs::read_to_string(path()).unwrap();
        assert!(!raw.contains("SECRET") && !raw.contains("PRIVATE"), "the list holds no key");
        // changing a machine keeps its key
        add(Machine { note: "x".into(), ..m("a", "h3", "u", 22) }, None).unwrap();
        assert_eq!(get("a").unwrap().host, "h3");
        remove("a").unwrap();
        assert!(get("a").is_none() && crate::ai::keys::get("ssh-a").is_none());
        assert!(remove("a").is_err());
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("ZOHARA_AGENT_KEYS_FILE");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
