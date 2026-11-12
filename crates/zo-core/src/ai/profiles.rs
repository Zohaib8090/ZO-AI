//! Profiles: saved sets of "provider, model, address, effort, pictures" that can be switched in one click, each with its own API
//! key in the system keyring (so one profile can use one OpenRouter account and another a different one). The chosen profile is the
//! `profile` of the current `Selection`; the profiles themselves are in `profiles.json`, without any keys.

use super::{keys, Selection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    /// Models of this profile that show in the model menu (so one click switches to that profile and model).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<String>,
    /// When it last changed (ms; 0 = from before this was kept). Set by `save`; the sync compares it between computers.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub updated: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn file() -> PathBuf {
    crate::audit::config_dir().join("profiles.json")
}

pub fn load() -> Vec<Profile> {
    std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Saves the list. A profile that is new or changed gets the time now; one that was removed leaves a tombstone for the sync.
pub fn save(all: &[Profile]) {
    let old = load();
    let now = crate::store::chats::now_millis();
    let mut out = all.to_vec();
    for p in &mut out {
        p.updated = match old.iter().find(|o| o.id == p.id) {
            Some(o) => {
                let mut same = o.clone();
                same.updated = p.updated;
                if same == *p { o.updated } else { now.max(o.updated.saturating_add(1)) }
            }
            None => now,
        };
    }
    for o in &old {
        if !all.iter().any(|p| p.id == o.id) {
            crate::sync::tombstone("profiles", &o.id, o.updated.max(1));
        }
    }
    save_as_is(&out);
}

/// Saves the list exactly as given (the sync stores a profile with the time it was changed on the other side).
pub(crate) fn save_as_is(all: &[Profile]) {
    let _ = std::fs::create_dir_all(crate::audit::config_dir());
    let tmp = file().with_extension("json.tmp");
    if let Ok(s) = serde_json::to_string_pretty(all) {
        if !crate::tenant::fits(&file(), s.len()) {
            return;
        }
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, file());
        }
    }
}

/// Changes the address and/or effort of a profile (`None` leaves it; an empty text takes the default again).
pub fn update(id: &str, base_url: Option<&str>, effort: Option<&str>) -> Result<(), String> {
    let mut all = load();
    let p = all.iter_mut().find(|p| p.id == id).ok_or("That profile is gone.")?;
    if let Some(u) = base_url {
        p.base_url = Some(u.trim().to_string()).filter(|u| !u.is_empty());
    }
    if let Some(e) = effort {
        p.effort = Some(e.trim().to_string()).filter(|e| !e.is_empty());
    }
    save(&all);
    Ok(())
}

/// The keyring entry of a provider's key: the profile's own when one is chosen, else the shared one of the provider.
pub fn key_id(sel: &Selection, provider_id: &str) -> String {
    match &sel.profile {
        Some(id) => format!("{provider_id}@{id}"),
        None => provider_id.to_string(),
    }
}

/// The key to use: the chosen profile's, and if that profile has none yet, the provider's shared one.
pub fn get_key(sel: &Selection, provider_id: &str) -> Option<String> {
    // the shared key of the provider only goes to the provider's own address: a profile (which can arrive by sync) that points
    // somewhere else must have a key of its own
    let shared_ok = sel.profile.is_some() && super::providers::find(provider_id).is_some_and(|p| p.key_fits(&sel.base_url_for(p)));
    keys::get(&key_id(sel, provider_id)).or_else(|| if shared_ok { keys::get(provider_id) } else { None })
}

/// Does this profile (or, without one, this provider) have a key of its own?
pub fn has_own_key(sel: &Selection, provider_id: &str) -> bool {
    keys::get(&key_id(sel, provider_id)).is_some()
}

fn new_id() -> String {
    format!("p{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
}

fn from_selection(id: String, name: &str, sel: &Selection) -> Profile {
    let base_url = sel.base_urls.get(&sel.provider).cloned();
    Profile { id, name: name.trim().to_string(), provider: sel.provider.clone(), model: sel.model.clone(), base_url, effort: sel.effort.clone(), vision: sel.vision, pinned: Vec::new(), updated: 0 }
}

/// Makes a profile from the current choices. The key currently in use is copied into the new profile, so it works at once.
pub fn create(name: &str, sel: &Selection) -> Result<Profile, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the profile a name.".into());
    }
    let mut all = load();
    if all.iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
        return Err(format!("A profile named \"{name}\" already exists."));
    }
    let p = from_selection(new_id(), name, sel);
    if let Some(k) = get_key(sel, &p.provider) {
        let _ = keys::set(&format!("{}@{}", p.provider, p.id), &k);
    }
    all.push(p.clone());
    save(&all);
    Ok(p)
}

/// Writes the current choices into the profile that is chosen (called when the model settings are saved).
pub fn update_active(sel: &Selection) {
    let Some(id) = &sel.profile else { return };
    let mut all = load();
    if let Some(p) = all.iter_mut().find(|p| &p.id == id) {
        let (name, pinned) = (p.name.clone(), std::mem::take(&mut p.pinned));
        *p = from_selection(id.clone(), &name, sel);
        p.pinned = pinned;
        save(&all);
    }
}

/// Puts a model of a profile in the model menu.
pub fn pin(profile_id: &str, model: &str) {
    let mut all = load();
    if let Some(p) = all.iter_mut().find(|p| p.id == profile_id) {
        if !model.is_empty() && !p.pinned.iter().any(|m| m == model) {
            p.pinned.push(model.to_string());
            save(&all);
        }
    }
}

/// Takes a model of a profile out of the model menu.
pub fn unpin(profile_id: &str, model: &str) {
    let mut all = load();
    if let Some(p) = all.iter_mut().find(|p| p.id == profile_id) {
        p.pinned.retain(|m| m != model);
        save(&all);
    }
}

/// The profile in use; when there is none, one is made from the current settings, named after the provider ("OpenRouter", "OpenRouter 2").
pub fn ensure_active(sel: &mut Selection, provider_name: &str) -> Result<String, String> {
    if let Some(id) = sel.profile.clone().filter(|id| load().iter().any(|p| &p.id == id)) {
        return Ok(id);
    }
    let taken: Vec<String> = load().into_iter().map(|p| p.name.to_lowercase()).collect();
    let name = (1..)
        .map(|n| if n == 1 { provider_name.to_string() } else { format!("{provider_name} {n}") })
        .find(|n| !taken.contains(&n.to_lowercase()))
        .unwrap_or_else(|| provider_name.to_string());
    let p = create(&name, sel)?;
    sel.profile = Some(p.id.clone());
    Ok(p.id)
}

pub fn rename(id: &str, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the profile a name.".into());
    }
    let mut all = load();
    if all.iter().any(|p| p.id != id && p.name.eq_ignore_ascii_case(name)) {
        return Err(format!("A profile named \"{name}\" already exists."));
    }
    if let Some(p) = all.iter_mut().find(|p| p.id == id) {
        p.name = name.to_string();
    }
    save(&all);
    Ok(())
}

/// Removes a profile and the key it kept in the keyring.
pub fn delete(id: &str) {
    let mut all = load();
    if let Some(p) = all.iter().find(|p| p.id == id) {
        keys::clear(&format!("{}@{}", p.provider, p.id));
    }
    all.retain(|p| p.id != id);
    save(&all);
    let mut sel = Selection::load();
    if sel.profile.as_deref() == Some(id) {
        sel.profile = None;
        sel.save();
    }
}

impl Selection {
    /// Switches to a profile: its provider, model, address, effort and picture setting become the current ones.
    pub fn apply_profile(&mut self, p: &Profile) {
        self.provider = p.provider.clone();
        self.model = p.model.clone();
        match &p.base_url {
            Some(u) => {
                self.base_urls.insert(p.provider.clone(), u.clone());
            }
            None => {
                self.base_urls.remove(&p.provider);
            }
        }
        self.effort = p.effort.clone();
        self.vision = p.vision;
        self.profile = Some(p.id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_keeps_its_choices_and_switching_applies_them() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-profiles-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        let mut sel = Selection { provider: "openrouter".into(), model: "x/y".into(), effort: Some("high".into()), ..Default::default() };
        sel.base_urls.insert("openrouter".into(), "https://example.com/v1".into());
        let a = create("Work", &sel).unwrap();
        assert!(create("work", &sel).is_err(), "names are unique, ignoring case");
        assert!(create("  ", &sel).is_err());
        let other = Selection { provider: "gemini".into(), model: "gemini-2.5-pro".into(), ..Default::default() };
        let b = create("Home", &other).unwrap();
        assert_eq!(load().len(), 2);

        let mut now = Selection::default();
        now.apply_profile(&a);
        assert_eq!((now.provider.as_str(), now.model.as_str(), now.effort.as_deref(), now.profile.as_deref()), ("openrouter", "x/y", Some("high"), Some(a.id.as_str())));
        assert_eq!(now.base_urls["openrouter"], "https://example.com/v1");
        now.apply_profile(&b);
        assert!(now.base_urls.get("gemini").is_none() && now.effort.is_none());
        assert_eq!(key_id(&now, "gemini"), format!("gemini@{}", b.id));
        assert_eq!(key_id(&Selection::default(), "gemini"), "gemini");

        // pinned models stay through updates
        pin(&b.id, "gemini-2.5-pro");
        pin(&b.id, "gemini-2.5-pro");
        pin(&b.id, "gemini-2.5-flash");
        assert_eq!(load().iter().find(|p| p.id == b.id).unwrap().pinned, ["gemini-2.5-pro", "gemini-2.5-flash"]);
        // saving the model settings while a profile is chosen writes back into it
        now.model = "gemini-2.5-flash".into();
        update_active(&now);
        assert_eq!(load().iter().find(|p| p.id == b.id).unwrap().pinned.len(), 2);
        unpin(&b.id, "gemini-2.5-pro");
        assert_eq!(load().iter().find(|p| p.id == b.id).unwrap().pinned, ["gemini-2.5-flash"]);
        let mut plain = Selection { provider: "openrouter".into(), model: "x/y".into(), ..Default::default() };
        let made = ensure_active(&mut plain, "OpenRouter").unwrap();
        assert_eq!(load().iter().find(|p| p.id == made).unwrap().name, "OpenRouter");
        assert_eq!(ensure_active(&mut plain, "OpenRouter").unwrap(), made, "an existing profile is kept");
        let mut another = Selection { provider: "openrouter".into(), ..Default::default() };
        let second = ensure_active(&mut another, "OpenRouter").unwrap();
        assert_eq!(load().iter().find(|p| p.id == second).unwrap().name, "OpenRouter 2");
        delete(&made);
        delete(&second);
        rename(&b.id, "Personal").unwrap();
        assert!(rename(&b.id, "work").is_err());
        now.save();
        delete(&b.id);
        assert_eq!(load().len(), 1);
        assert!(Selection::load().profile.is_none(), "deleting the chosen profile unchooses it");
        delete(&a.id);
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
