//! The product was called ZO-AI and is now Zohara Chat. Everything an existing install already has on disk or in its
//! environment used the old name; this module is how the new code still finds it.
//!
//! The rule: read the OLD place when the new one does not exist yet, copy it to the new place once, and never delete the old
//! one (a person can go back to an older version without losing anything).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The old folder / service name (config, data and runtime folders, the keyring service).
pub const OLD_NAME: &str = "zo-ai";
/// The new one.
pub const NEW_NAME: &str = "zohara-chat";

/// An environment variable under its new name, else under the old one (empty counts as unset). The new name wins.
pub fn env_os(new: &str, old: &str) -> Option<OsString> {
    [new, old].iter().filter_map(|n| std::env::var_os(n)).find(|v| !v.is_empty())
}

/// Like `std::env::var`, with the old name as the fallback.
pub fn var(new: &str, old: &str) -> Result<String, std::env::VarError> {
    env(new, old).ok_or(std::env::VarError::NotPresent)
}

/// Like `env_os`, as text.
pub fn env(new: &str, old: &str) -> Option<String> {
    env_os(new, old).and_then(|v| v.into_string().ok())
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
        let (old, new) = (d.join("zo-ai"), d.join("zohara-chat"));
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
        let (old, new) = (d.join("zo-ai"), d.join("zohara-chat"));
        assert_eq!(migrate_dir(&old, &new), new);
        assert!(!new.exists() && !old.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env_prefers_the_new_name_and_falls_back_to_the_old() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ZO_LEGACY_TEST_NEW");
        std::env::remove_var("ZO_LEGACY_TEST_OLD");
        assert_eq!(env("ZO_LEGACY_TEST_NEW", "ZO_LEGACY_TEST_OLD"), None);
        std::env::set_var("ZO_LEGACY_TEST_OLD", "old");
        assert_eq!(env("ZO_LEGACY_TEST_NEW", "ZO_LEGACY_TEST_OLD").as_deref(), Some("old"));
        std::env::set_var("ZO_LEGACY_TEST_NEW", "");
        assert_eq!(env("ZO_LEGACY_TEST_NEW", "ZO_LEGACY_TEST_OLD").as_deref(), Some("old"), "an empty new variable does not hide the old one");
        std::env::set_var("ZO_LEGACY_TEST_NEW", "new");
        assert_eq!(env("ZO_LEGACY_TEST_NEW", "ZO_LEGACY_TEST_OLD").as_deref(), Some("new"));
        std::env::remove_var("ZO_LEGACY_TEST_NEW");
        std::env::remove_var("ZO_LEGACY_TEST_OLD");
    }
}
