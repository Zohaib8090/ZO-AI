//! The product was called ZO-AI, then Zohara Chat, and is now Zohara Agent. Everything an existing install already has on disk or in
//! its environment used an older name; this module is how the new code still finds it. An install can be at ANY of the three stages:
//! the new name wins, then the middle one (Zohara Chat), then the first (ZO-AI).
//!
//! The rule: read the OLD place when the new one does not exist yet, copy it to the new place once, and never delete the old
//! one (a person can go back to an older version without losing anything).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The current folder / service name (config, data and runtime folders, the keyring service).
pub const NAME: &str = "zohara-agent";
/// The names before it, newest first: Zohara Chat, then ZO-AI.
pub const OLD_NAMES: [&str; 2] = ["zohara-chat", "zo-ai"];

/// Environment variable prefixes, preferred one first. `ZOHARA_AI_` never shipped as such but is accepted too.
pub const ENV_PREFIXES: [&str; 4] = ["ZOHARA_AGENT_", "ZOHARA_CHAT_", "ZOHARA_AI_", "ZO_AI_"];

/// The first of these variables that is set and not empty (the earlier in the list wins).
pub fn first_env_os(names: &[String]) -> Option<OsString> {
    names.iter().filter_map(|n| std::env::var_os(n)).find(|v| !v.is_empty())
}

/// The variable `<prefix><suffix>` under the new prefix, else under the older ones, else under `extra` (names that did not follow the
/// pattern, such as `ZOAI_CLAUDE_BIN`). Empty counts as unset. The new name wins.
pub fn env_os_with(suffix: &str, extra: &[&str]) -> Option<OsString> {
    let names: Vec<String> = ENV_PREFIXES.iter().map(|p| format!("{p}{suffix}")).chain(extra.iter().map(|s| s.to_string())).collect();
    first_env_os(&names)
}

/// `env_os_with` without irregular names.
pub fn env_os(suffix: &str) -> Option<OsString> {
    env_os_with(suffix, &[])
}

/// Like `std::env::var`, with the older names as the fallback.
pub fn var(suffix: &str) -> Result<String, std::env::VarError> {
    env(suffix).ok_or(std::env::VarError::NotPresent)
}

/// Like `env_os`, as text.
pub fn env(suffix: &str) -> Option<String> {
    env_os(suffix).and_then(|v| v.into_string().ok())
}

/// `base/<name>` for the current name.
pub fn current_under(base: &Path) -> PathBuf {
    base.join(NAME)
}

/// `base/<name>` for each older name, newest first.
pub fn old_under(base: &Path) -> Vec<PathBuf> {
    OLD_NAMES.iter().map(|n| base.join(n)).collect()
}

/// Makes `base/<current>` exist from the newest older folder there is (`base/zohara-chat`, else `base/zo-ai`), once.
pub fn migrate_under(base: &Path) -> PathBuf {
    migrate_dir_any(&old_under(base), &current_under(base))
}

/// Copies a folder (files, folders, symbolic links, permissions) to a place that does not exist yet. The copy is made next to
/// the target under a temporary name and renamed into place, so a crash never leaves a half copy that looks complete.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return std::os::unix::fs::symlink(std::fs::read_link(from)?, to);
    }
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_tree(&e.path(), &to.join(e.file_name()))?;
        }
        return std::fs::set_permissions(to, std::fs::Permissions::from_mode(meta.permissions().mode() & 0o7777));
    }
    std::fs::copy(from, to)?;
    std::fs::set_permissions(to, std::fs::Permissions::from_mode(meta.permissions().mode() & 0o7777))
}

/// Makes `new` exist from the first of `olds` that is a folder (newest first), once; see `migrate_dir`.
pub fn migrate_dir_any(olds: &[PathBuf], new: &Path) -> PathBuf {
    if new.exists() {
        return new.to_path_buf();
    }
    match olds.iter().find(|o| o.is_dir()) {
        Some(old) => migrate_dir(old, new),
        None => new.to_path_buf(),
    }
}

/// Makes `new` exist from `old`, once. Returns the folder to use: `new` when it exists or was just made, `old` when the copy
/// failed (the person keeps working on what they have), `new` when there is nothing to migrate.
pub fn migrate_dir(old: &Path, new: &Path) -> PathBuf {
    if new.exists() || !old.is_dir() {
        return new.to_path_buf();
    }
    let mut tmp_name = new.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    tmp_name.push(format!(".migrating-{}", std::process::id()));
    let tmp = new.with_file_name(tmp_name);
    let _ = std::fs::remove_dir_all(&tmp);
    if let Some(parent) = new.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match copy_tree(old, &tmp).and_then(|_| std::fs::rename(&tmp, new)) {
        Ok(()) => new.to_path_buf(),
        Err(_) => {
            let _ = std::fs::remove_dir_all(&tmp);
            // another process may have won the race: then the new folder exists now
            if new.exists() {
                new.to_path_buf()
            } else {
                old.to_path_buf()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zo-legacy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn migrate_copies_once_and_keeps_the_old_folder() {
        let d = temp("copy");
        let (old, new) = (d.join("zo-ai"), d.join("zohara-agent"));
        std::fs::create_dir_all(old.join("sub")).unwrap();
        std::fs::write(old.join("keys.json"), "{\"a\":1}").unwrap();
        std::fs::write(old.join("sub/x"), "x").unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(old.join("keys.json"), std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(migrate_dir(&old, &new), new);
        assert_eq!(std::fs::read_to_string(new.join("keys.json")).unwrap(), "{\"a\":1}");
        assert_eq!(std::fs::read_to_string(new.join("sub/x")).unwrap(), "x");
        assert_eq!(std::fs::metadata(&new).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(new.join("keys.json")).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(old.join("keys.json").exists(), "the old folder is kept");
        // the second run must not copy again over what changed since
        std::fs::write(new.join("keys.json"), "{\"a\":2}").unwrap();
        std::fs::write(old.join("later"), "l").unwrap();
        assert_eq!(migrate_dir(&old, &new), new);
        assert_eq!(std::fs::read_to_string(new.join("keys.json")).unwrap(), "{\"a\":2}");
        assert!(!new.join("later").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn migrate_with_nothing_to_move_is_a_no_op() {
        let d = temp("none");
        let (old, new) = (d.join("zo-ai"), d.join("zohara-agent"));
        assert_eq!(migrate_dir(&old, &new), new);
        assert!(!new.exists() && !old.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn migrate_any_takes_the_newest_old_folder() {
        let d = temp("any");
        let (chat, ai, new) = (d.join("zohara-chat"), d.join("zo-ai"), d.join("zohara-agent"));
        // an install still at ZO-AI
        std::fs::create_dir_all(&ai).unwrap();
        std::fs::write(ai.join("f"), "from-ai").unwrap();
        assert_eq!(migrate_dir_any(&[chat.clone(), ai.clone()], &new), new);
        assert_eq!(std::fs::read_to_string(new.join("f")).unwrap(), "from-ai");
        assert!(ai.join("f").exists());
        std::fs::remove_dir_all(&new).unwrap();
        // an install that went ZO-AI -> Zohara Chat: the Zohara Chat copy is the current one
        std::fs::create_dir_all(&chat).unwrap();
        std::fs::write(chat.join("f"), "from-chat").unwrap();
        assert_eq!(migrate_dir_any(&[chat.clone(), ai.clone()], &new), new);
        assert_eq!(std::fs::read_to_string(new.join("f")).unwrap(), "from-chat");
        assert!(chat.join("f").exists() && ai.join("f").exists(), "both old folders are kept");
        // an install that is already at the new name is left alone
        std::fs::write(chat.join("g"), "later").unwrap();
        assert_eq!(migrate_dir_any(&[chat, ai], &new), new);
        assert!(!new.join("g").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn migrate_any_with_nothing_to_move_is_a_no_op() {
        let d = temp("anynone");
        let new = d.join("zohara-agent");
        assert_eq!(migrate_dir_any(&[d.join("zohara-chat"), d.join("zo-ai")], &new), new);
        assert!(!new.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_prefers_the_new_name_then_each_older_one() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let all = ["ZOHARA_AGENT_LEGACYT", "ZOHARA_CHAT_LEGACYT", "ZOHARA_AI_LEGACYT", "ZO_AI_LEGACYT", "ZOAI_LEGACYT_ODD"];
        for n in all {
            std::env::remove_var(n);
        }
        assert_eq!(env("LEGACYT"), None);
        std::env::set_var("ZO_AI_LEGACYT", "zo-ai");
        assert_eq!(env("LEGACYT").as_deref(), Some("zo-ai"));
        std::env::set_var("ZOHARA_AI_LEGACYT", "zohara-ai");
        assert_eq!(env("LEGACYT").as_deref(), Some("zohara-ai"));
        std::env::set_var("ZOHARA_CHAT_LEGACYT", "chat");
        assert_eq!(env("LEGACYT").as_deref(), Some("chat"));
        std::env::set_var("ZOHARA_AGENT_LEGACYT", "");
        assert_eq!(env("LEGACYT").as_deref(), Some("chat"), "an empty new variable does not hide an old one");
        std::env::set_var("ZOHARA_AGENT_LEGACYT", "agent");
        assert_eq!(env("LEGACYT").as_deref(), Some("agent"));
        assert_eq!(var("LEGACYT").unwrap(), "agent");
        // a name outside the pattern
        for n in all {
            std::env::remove_var(n);
        }
        assert_eq!(env_os_with("LEGACYT", &["ZOAI_LEGACYT_ODD"]), None);
        std::env::set_var("ZOAI_LEGACYT_ODD", "odd");
        assert_eq!(env_os_with("LEGACYT", &["ZOAI_LEGACYT_ODD"]).unwrap(), "odd");
        std::env::set_var("ZO_AI_LEGACYT", "zo-ai");
        assert_eq!(env_os_with("LEGACYT", &["ZOAI_LEGACYT_ODD"]).unwrap(), "zo-ai", "the regular names come before the odd one");
        for n in all {
            std::env::remove_var(n);
        }
    }
}
