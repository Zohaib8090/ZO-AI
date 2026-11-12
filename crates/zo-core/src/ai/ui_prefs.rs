//! What the person chose to see in the model pickers, the same on the desktop and the web (it is one of the synced kinds, `settings`):
//! `hidden_providers` (providers that do not show in the pickers; pinned models still show first) and `provider_prefs` (small
//! per-provider UI choices, for example the order). The current provider and model are NOT here: they stay per device.
//! File: `ui-prefs.json` in the config folder.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The biggest the file (and so a synced document) may be.
pub const MAX_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Prefs {
    #[serde(default)]
    pub hidden_providers: Vec<String>,
    #[serde(default)]
    pub provider_prefs: BTreeMap<String, Value>,
    /// When it last changed (ms); 0 = never changed.
    #[serde(default)]
    pub updated: u64,
}

fn file() -> std::path::PathBuf {
    crate::audit::config_dir().join("ui-prefs.json")
}

pub fn load() -> Prefs {
    std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Writes the preferences exactly as given (the sync applies a document with its own time this way).
pub(crate) fn save_as_is(p: &Prefs) -> Result<(), String> {
    let s = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    if s.len() > MAX_BYTES {
        return Err("The preferences are too big.".into());
    }
    if !crate::tenant::fits(&file(), s.len()) {
        return Err("There is no room left in this account.".into());
    }
    let _ = std::fs::create_dir_all(crate::audit::config_dir());
    let tmp = file().with_extension("json.tmp");
    std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file()).map_err(|e| e.to_string())
}

fn clean_id(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| s.to_string())
}

/// Checks a document's shape and size; the same rules for the person's own edits and for a synced document.
pub fn clean(mut p: Prefs) -> Result<Prefs, String> {
    let mut seen = Vec::new();
    for h in &p.hidden_providers {
        let id = clean_id(h).ok_or("A hidden provider has a name that is not allowed.")?;
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    if seen.len() > 200 || p.provider_prefs.len() > 200 {
        return Err("Too many entries in the preferences.".into());
    }
    p.hidden_providers = seen;
    for (k, v) in &p.provider_prefs {
        if clean_id(k).is_none() || !v.is_object() {
            return Err("The provider preferences are not valid.".into());
        }
    }
    if serde_json::to_string(&p).map_or(true, |s| s.len() > MAX_BYTES) {
        return Err("The preferences are too big.".into());
    }
    Ok(p)
}

fn edit(f: impl FnOnce(&mut Prefs)) -> Result<Prefs, String> {
    let old = load();
    let mut p = old.clone();
    f(&mut p);
    let mut p = clean(p)?;
    if p == old {
        return Ok(old);
    }
    p.updated = crate::store::chats::now_millis().max(old.updated.saturating_add(1));
    save_as_is(&p)?;
    Ok(p)
}

/// The providers that do not show in the pickers (replaces the list).
pub fn set_hidden(list: Vec<String>) -> Result<Prefs, String> {
    edit(|p| p.hidden_providers = list)
}

/// Small UI choices of one provider (an object; `null` or an empty object forgets them).
pub fn set_provider_pref(provider: &str, value: Value) -> Result<Prefs, String> {
    let provider = provider.to_string();
    edit(move |p| {
        if value.as_object().map_or(true, |o| o.is_empty()) {
            p.provider_prefs.remove(&provider);
        } else {
            p.provider_prefs.insert(provider, value);
        }
    })
}

/// The document that travels (`settings:main`).
pub fn doc(p: &Prefs) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}
