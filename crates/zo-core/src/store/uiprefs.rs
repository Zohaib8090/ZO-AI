//! Small interface preferences that follow the person between devices (the sync carries this file as the "settings" kind).
//! Today: which providers show in the model pickers. The current choice of provider and model is not here: it stays per device.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Prefs {
    /// Provider ids that are left out of the model pickers.
    #[serde(default)]
    pub hidden_providers: Vec<String>,
    /// When it was last changed (seconds), so the sync can tell which side is newer.
    #[serde(default)]
    pub updated: u64,
}

static LOCK: Mutex<()> = Mutex::new(());

fn path() -> PathBuf {
    crate::audit::config_dir().join("uiprefs.json")
}

pub fn load() -> Prefs {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn save(p: &Prefs) {
    let path = path();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
    let tmp = path.with_extension("json.tmp");
    if let Ok(t) = serde_json::to_string_pretty(p) {
        if !crate::tenant::fits(&path, t.len()) {
            return;
        }
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// The providers that are left out of the pickers.
pub fn hidden_providers() -> Vec<String> {
    load().hidden_providers
}

pub fn set_hidden_providers(ids: Vec<String>) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = load();
    let mut ids = ids;
    ids.sort();
    ids.dedup();
    if p.hidden_providers != ids {
        p.hidden_providers = ids;
        p.updated = now();
        save(&p);
    }
}

pub fn is_provider_hidden(id: &str) -> bool {
    hidden_providers().iter().any(|h| h == id)
}

/// Shows or hides one provider in the pickers.
pub fn set_provider_hidden(id: &str, hidden: bool) {
    let mut v = hidden_providers();
    v.retain(|h| h != id);
    if hidden {
        v.push(id.to_string());
    }
    set_hidden_providers(v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiding_and_showing_a_provider() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-uiprefs-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        assert!(!is_provider_hidden("groq"));
        set_provider_hidden("groq", true);
        set_provider_hidden("groq", true);
        assert_eq!(hidden_providers(), vec!["groq".to_string()]);
        set_provider_hidden("groq", false);
        assert!(hidden_providers().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
