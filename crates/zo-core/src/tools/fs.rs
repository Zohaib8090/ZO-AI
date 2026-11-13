use super::expand;
use std::fs;
use std::path::Path;

const MAX_READ: u64 = 1_000_000;

pub fn read(path: &str) -> Result<String, String> {
    let p = expand(path);
    super::guard_read(&p)?;
    let meta = fs::metadata(&p).map_err(|e| format!("{path}: {e}"))?;
    if meta.is_dir() {
        return Err(format!("{path} is a folder; use zohara_fs_list"));
    }
    if meta.len() > MAX_READ {
        return Err(format!("{path} is {} bytes; the limit is {MAX_READ}", meta.len()));
    }
    let bytes = fs::read(&p).map_err(|e| format!("{path}: {e}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{path} is not text"))
}

pub fn write(path: &str, content: &str, append: bool) -> Result<String, String> {
    use std::io::Write;
    let p = expand(path);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{path}: {e}"))?;
    }
    let mut f = fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&p).map_err(|e| format!("{path}: {e}"))?;
    f.write_all(content.as_bytes()).map_err(|e| format!("{path}: {e}"))?;
    Ok(format!("{} {} bytes to {path}", if append { "Appended" } else { "Wrote" }, content.len()))
}

fn walk(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<String>, limit: usize) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= limit {
            return;
        }
        let path = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push(format!("{}{}", path.display(), if is_dir { "/" } else { "" }));
        if is_dir && depth < max_depth {
            walk(&path, depth + 1, max_depth, out, limit);
        }
    }
}

pub fn list(path: &str, recursive: bool) -> Result<String, String> {
    let p = expand(path);
    super::guard_read(&p)?;
    if !p.is_dir() {
        return Err(format!("{path} is not a folder"));
    }
    let mut out = Vec::new();
    walk(&p, 1, if recursive { 3 } else { 1 }, &mut out, 500);
    if out.len() >= 500 {
        out.push("… (stopped at 500 entries)".into());
    }
    Ok(if out.is_empty() { "(empty)".into() } else { out.join("\n") })
}

/// `*` matches anything; without one the pattern is a substring.
fn matches(name: &str, pattern: &str) -> bool {
    let (n, p) = (name.to_lowercase(), pattern.to_lowercase());
    if !p.contains('*') {
        return n.contains(&p);
    }
    let parts: Vec<&str> = p.split('*').collect();
    let mut rest = n.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match rest.find(part) {
            Some(at) if !(i == 0 && at != 0) => rest = &rest[at + part.len()..],
            _ => return false,
        }
    }
    parts.last().map_or(true, |l| l.is_empty() || n.ends_with(l))
}

pub fn search(pattern: &str, path: &str) -> Result<String, String> {
    let root = expand(path);
    super::guard_read(&root)?;
    let mut found = Vec::new();
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            visited += 1;
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if matches(&name, pattern) {
                found.push(e.path().display().to_string());
                if found.len() >= 50 {
                    return Ok(found.join("\n") + "\n… (first 50 matches)");
                }
            }
            // hidden folders (caches, .git) are skipped to keep it fast
            if is_dir && !name.starts_with('.') && visited < 200_000 {
                stack.push(e.path());
            }
        }
    }
    Ok(if found.is_empty() { "No matches".into() } else { found.join("\n") })
}

pub fn mkdir(path: &str) -> Result<String, String> {
    fs::create_dir_all(expand(path)).map_err(|e| format!("{path}: {e}"))?;
    Ok(format!("Created {path}"))
}

pub fn rm(path: &str, recursive: bool) -> Result<String, String> {
    let p = expand(path);
    let canon = fs::canonicalize(&p).map_err(|e| format!("{path}: {e}"))?;
    let home = std::env::var("HOME").ok().and_then(|h| fs::canonicalize(h).ok());
    if canon == Path::new("/") || home.as_deref() == Some(canon.as_path()) || canon.components().count() <= 2 {
        return Err(format!("refusing to delete {}", canon.display()));
    }
    if canon.is_dir() {
        if !recursive {
            return Err(format!("{path} is a folder; pass recursive=true to delete it and its contents"));
        }
        fs::remove_dir_all(&canon)
    } else {
        fs::remove_file(&canon)
    }
    .map_err(|e| format!("{path}: {e}"))?;
    Ok(format!("Deleted {}", canon.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("zo-core-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_read_append_list_search_delete() {
        let d = temp("fs");
        let f = d.join("sub/note.txt");
        let fs_ = f.to_str().unwrap();
        write(fs_, "hello", false).unwrap();
        write(fs_, " world", true).unwrap();
        assert_eq!(read(fs_).unwrap(), "hello world");
        assert!(list(d.to_str().unwrap(), true).unwrap().contains("note.txt"));
        assert!(search("NOTE*.TXT", d.to_str().unwrap()).unwrap().contains("note.txt"));
        assert_eq!(search("nothing-here", d.to_str().unwrap()).unwrap(), "No matches");
        assert!(rm(d.join("sub").to_str().unwrap(), false).is_err(), "a folder needs recursive");
        rm(d.join("sub").to_str().unwrap(), true).unwrap();
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn secret_files_are_refused() {
        assert!(read("/proc/self/environ").is_err());
        assert!(read("/proc/1/environ").is_err());
        assert!(read("~/.ssh/id_rsa").is_err());
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = temp("guard");
        std::env::set_var("XDG_CONFIG_HOME", &d);
        let keys = d.join("zohara-agent/keys.json");
        fs::create_dir_all(keys.parent().unwrap()).unwrap();
        fs::write(&keys, "{}").unwrap();
        assert!(read(keys.to_str().unwrap()).is_err());
        assert!(list(keys.parent().unwrap().to_str().unwrap(), false).is_err());
        std::env::remove_var("XDG_CONFIG_HOME");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn it_will_not_delete_the_root_or_home() {
        assert!(rm("/", true).is_err());
        assert!(rm("~", true).is_err());
        assert!(rm("/usr", true).is_err());
    }

    #[test]
    fn name_matching() {
        assert!(matches("Photo.PNG", "*.png"));
        assert!(matches("report-final.pdf", "report*pdf"));
        assert!(!matches("report.pdf", "*.png"));
        assert!(matches("abc", "b"));
        assert!(!matches("xabc", "abc*z"));
    }
}
