//! Phase 1 of "full control" (docs/FULL-CONTROL.md): the read-only operations the web may ask of THIS computer through the link,
//! beyond the settings questions: `sys_info`, `fs_list`, `fs_read` and `screenshot`.
//!
//! Everything here is built for input that comes from the network:
//! * paths are resolved to a real path (symbolic links followed) and must stay inside the person's home folder; the places that hold
//!   secrets (`guard_read` and the secret names of the permission policy) are refused, and left out of folder listings;
//! * sizes are capped (500 entries, 256 KB of text, a screenshot that fits one relay post);
//! * nothing here changes anything on the computer (a screenshot file is made in the app's data folder and deleted at once).
//!
//! The link (`link.rs`) asks the person ON THIS COMPUTER before `fs_list`, `fs_read` and `screenshot` run (every time, no trusted
//! session in this phase); `sys_info` shows no personal content and runs without asking.

use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The operations of this module.
pub const OPS: &[&str] = &["sys_info", "fs_list", "fs_read", "screenshot"];
/// Operations that ask the person on this computer first.
pub fn needs_approval(op: &str) -> bool {
    matches!(op, "fs_list" | "fs_read" | "screenshot")
}

pub const MAX_PATH_CHARS: usize = 1024;
pub const MAX_ENTRIES: usize = 500;
pub const MAX_READ_BYTES: usize = 256 * 1024;
/// A screenshot answer (base64) must fit into one relay post (1 MB) with room to spare.
pub const MAX_SHOT_B64: usize = 700_000;

fn home() -> Result<PathBuf, String> {
    let h = std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or("This computer has no home folder to show.")?;
    std::fs::canonicalize(h).map_err(|_| "The home folder is not available.".to_string())
}

/// Text that is safe to show in a dialog or a list: no control or direction characters, cut to `max` characters.
pub fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')).take(max).collect()
}

/// True when a path below the home folder names a place of secrets (the same names the permission policy keeps shell commands away from).
fn secret_name(rel: &Path) -> bool {
    let low = rel.to_string_lossy().to_ascii_lowercase();
    crate::policy::path_has_secret_part(&low)
}

/// Turns what the web sent into a real path inside the home folder, or says why not. `~`, `~/x`, a path below home, or a path
/// relative to home; empty means home. Symbolic links are followed first, so a link that leads out of home is refused.
pub fn resolve(raw: &str) -> Result<PathBuf, String> {
    if raw.chars().count() > MAX_PATH_CHARS || raw.chars().any(char::is_control) {
        return Err("That path is not valid.".into());
    }
    let home = home()?;
    let t = raw.trim();
    let joined = if t.is_empty() || t == "~" {
        home.clone()
    } else if let Some(rest) = t.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(t).is_absolute() {
        PathBuf::from(t)
    } else {
        home.join(t)
    };
    let real = std::fs::canonicalize(&joined).map_err(|_| "That file or folder does not exist.".to_string())?;
    if !real.starts_with(&home) {
        return Err("Only your home folder can be reached from the web.".into());
    }
    let rel = real.strip_prefix(&home).unwrap_or(&real);
    if secret_name(rel) || crate::tools::guard_read(&real).is_err() {
        return Err("That place holds secrets (keys, sign-ins, Zohara Chat's own settings), so it is not available from the web.".into());
    }
    Ok(real)
}

/// `~/x` for a path in home (what the person recognises).
pub fn shown(p: &Path) -> String {
    match home() {
        Ok(h) => match p.strip_prefix(&h) {
            Ok(r) if r.as_os_str().is_empty() => "~".to_string(),
            Ok(r) => format!("~/{}", clean(&r.to_string_lossy(), 300)),
            Err(_) => clean(&p.to_string_lossy(), 300),
        },
        Err(_) => clean(&p.to_string_lossy(), 300),
    }
}

/// A folder's entries: folders first, then files, by name; at most `MAX_ENTRIES`; entries that are secret places are left out.
pub fn list_dir(dir: &Path) -> Result<Value, String> {
    let meta = std::fs::metadata(dir).map_err(|_| "That folder cannot be read.".to_string())?;
    if !meta.is_dir() {
        return Err("That is a file, not a folder.".into());
    }
    let home = home()?;
    let rd = std::fs::read_dir(dir).map_err(|e| format!("That folder cannot be opened ({}).", e.kind()))?;
    let mut items: Vec<(bool, String, Value)> = Vec::new();
    let mut total = 0usize;
    for ent in rd.flatten() {
        total += 1;
        if total > 20_000 {
            break;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        let path = ent.path();
        let rel = path.strip_prefix(&home).unwrap_or(&path).to_path_buf();
        if secret_name(&rel) || crate::tools::guard_read(&path).is_err() {
            continue;
        }
        let Ok(lm) = std::fs::symlink_metadata(&path) else { continue };
        let ft = lm.file_type();
        let (kind, is_dir) = if ft.is_symlink() {
            // a link is shown as a link; opening it goes through `resolve` again, which refuses a way out of home
            match std::fs::metadata(&path) {
                Ok(m) if m.is_dir() => ("link-dir", true),
                _ => ("link", false),
            }
        } else if ft.is_dir() {
            ("dir", true)
        } else if ft.is_file() {
            ("file", false)
        } else {
            ("other", false)
        };
        let mtime = lm.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let shown_name = clean(&name, 200);
        items.push((is_dir, shown_name.to_lowercase(), json!({"name": shown_name, "kind": kind, "size": if ft.is_file() { lm.len() } else { 0 }, "mtime": mtime})));
    }
    items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let truncated = items.len() > MAX_ENTRIES;
    items.truncate(MAX_ENTRIES);
    let parent = dir.parent().filter(|p| p.starts_with(&home) && resolve(&p.to_string_lossy()).is_ok()).map(shown);
    Ok(json!({"ok": true, "path": shown(dir), "parent": parent, "entries": items.into_iter().map(|i| i.2).collect::<Vec<_>>(), "truncated": truncated}))
}

/// A text file, at most `MAX_READ_BYTES` of it. Folders, special files and anything that is not UTF-8 text are refused.
pub fn read_text(file: &Path) -> Result<Value, String> {
    // O_NONBLOCK so that a pipe with no writer does not hang the open; O_NOFOLLOW and O_NOCTTY for good measure
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY).open(file).map_err(|e| format!("That file cannot be opened ({}).", e.kind()))?;
    // the type is checked on the opened file, not on the name
    let meta = f.metadata().map_err(|_| "That file cannot be read.".to_string())?;
    if !meta.is_file() {
        return Err("That is not a regular file.".into());
    }
    let mut buf = Vec::new();
    f.take(MAX_READ_BYTES as u64 + 1).read_to_end(&mut buf).map_err(|_| "That file cannot be read.".to_string())?;
    let truncated = buf.len() > MAX_READ_BYTES;
    buf.truncate(MAX_READ_BYTES);
    if buf.contains(&0) {
        return Err("That looks like a binary file. Only text files can be shown here.".into());
    }
    let text = match String::from_utf8(buf) {
        Ok(t) => t,
        // a cut inside a multi-byte character is fine; anything else is not text
        Err(e) if truncated && e.utf8_error().error_len().is_none() => {
            let v = e.utf8_error().valid_up_to();
            String::from_utf8_lossy(&e.as_bytes()[..v]).into_owned()
        }
        Err(_) => return Err("That file is not UTF-8 text. Only text files can be shown here.".into()),
    };
    Ok(json!({"ok": true, "path": shown(file), "size": meta.len(), "truncated": truncated, "text": text}))
}

/// The answer to `sys_info`: the same facts the `system_info` tool gives, as text.
pub fn sys_info() -> Result<Value, String> {
    let text = crate::tools::system::info()?;
    let up = std::fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok())).unwrap_or(0.0) as u64;
    Ok(json!({"ok": true, "text": text, "uptime_secs": up, "os": std::env::consts::OS}))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn b64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Takes a screenshot of the screen and returns it as a data answer: a JPEG no wider than 1280 pixels when ImageMagick is there,
/// else the PNG if it is small enough. The files made on the way are deleted before this returns.
pub fn screenshot() -> Result<Value, String> {
    let png = PathBuf::from(crate::tools::system::screenshot()?);
    let jpg = png.with_extension("web.jpg");
    let result = (|| {
        let mut bytes: Option<(Vec<u8>, &'static str)> = None;
        for prog in ["magick", "convert"] {
            let mut c = std::process::Command::new(prog);
            c.arg(&png).args(["-resize", "1280x1280>", "-quality", "65"]).arg(&jpg);
            if let Ok(o) = crate::exec::run(&mut c, Duration::from_secs(20)) {
                if o.code == Some(0) {
                    if let Ok(b) = std::fs::read(&jpg) {
                        bytes = Some((b, "image/jpeg"));
                        break;
                    }
                }
            }
        }
        let (data, mime) = match bytes {
            Some(b) => b,
            None => (std::fs::read(&png).map_err(|_| "The screenshot could not be read.".to_string())?, "image/png"),
        };
        let enc = b64(&data);
        if enc.len() > MAX_SHOT_B64 {
            return Err("The screenshot is too big to send (install ImageMagick on the computer to shrink it).".to_string());
        }
        Ok(json!({"ok": true, "mime": mime, "bytes": data.len(), "data": enc}))
    })();
    let _ = std::fs::remove_file(&png);
    let _ = std::fs::remove_file(&jpg);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home folder of its own for the test (HOME is process-global: the lock is shared with the other tests that set the environment).
    fn home_sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, PathBuf, Option<std::ffi::OsString>) {
        let g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = PathBuf::from(format!("/tmp/zc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home/docs")).unwrap();
        let old = std::env::var_os("HOME");
        std::env::set_var("HOME", dir.join("home"));
        (g, std::fs::canonicalize(&dir).unwrap(), old)
    }
    fn restore(old: Option<std::ffi::OsString>, dir: &Path) {
        match old {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foob"), "Zm9vYg==");
        assert_eq!(crate::attach::decode_b64(&b64(&[0, 255, 7, 8, 9, 200])).unwrap(), vec![0, 255, 7, 8, 9, 200]);
    }

    #[test]
    fn only_the_home_folder_can_be_reached_and_links_out_of_it_are_refused() {
        let (_g, dir, old) = home_sandbox("resolve");
        std::fs::write(dir.join("home/docs/a.txt"), "hello").unwrap();
        std::fs::write(dir.join("outside.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(dir.join("outside.txt"), dir.join("home/docs/link.txt")).unwrap();
        std::os::unix::fs::symlink("/etc", dir.join("home/etc-link")).unwrap();
        let home = dir.join("home");
        assert_eq!(resolve("").unwrap(), home);
        assert_eq!(resolve("~").unwrap(), home);
        assert_eq!(resolve("~/docs").unwrap(), home.join("docs"));
        assert_eq!(resolve("docs/a.txt").unwrap(), home.join("docs/a.txt"));
        assert_eq!(resolve(&home.join("docs").to_string_lossy()).unwrap(), home.join("docs"));
        for bad in ["/etc/passwd", "/", "..", "../outside.txt", "docs/../../outside.txt", "~/../outside.txt", "docs/link.txt", "etc-link", "etc-link/passwd", "/proc/self/environ"] {
            assert!(resolve(bad).is_err(), "{bad} must be refused");
        }
        assert!(resolve("nope").unwrap_err().contains("does not exist"));
        assert!(resolve("a\0b").is_err());
        assert!(resolve(&"x".repeat(2000)).is_err());
        restore(old, &dir);
    }

    #[test]
    fn secret_places_are_refused_and_left_out_of_listings() {
        let (_g, dir, old) = home_sandbox("secret");
        let h = dir.join("home");
        for d in [".ssh", ".gnupg", ".config/zohara-chat", ".config/zo-ai", ".claude", ".aws"] {
            std::fs::create_dir_all(h.join(d)).unwrap();
            std::fs::write(h.join(d).join("f"), "x").unwrap();
        }
        std::fs::write(h.join("docs/my-token.txt"), "x").unwrap();
        std::fs::write(h.join("docs/notes.txt"), "x").unwrap();
        std::fs::write(h.join(".bash_history"), "x").unwrap();
        for bad in [".ssh", ".ssh/f", ".gnupg", ".config/zohara-chat/f", ".config/zo-ai/f", ".claude", ".aws/f", "docs/my-token.txt", ".bash_history"] {
            assert!(resolve(bad).is_err(), "{bad} must be refused");
        }
        let l = list_dir(&h).unwrap();
        let names: Vec<&str> = l["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"docs"));
        for hidden in [".ssh", ".gnupg", ".claude", ".aws", ".bash_history"] {
            assert!(!names.contains(&hidden), "{hidden} must not be listed");
        }
        let d = list_dir(&h.join("docs")).unwrap();
        let names: Vec<&str> = d["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["notes.txt"]);
        restore(old, &dir);
    }

    #[test]
    fn listing_sorts_folders_first_caps_entries_and_gives_the_parent() {
        let (_g, dir, old) = home_sandbox("list");
        let d = dir.join("home/docs");
        std::fs::create_dir_all(d.join("Zeta")).unwrap();
        std::fs::write(d.join("b.txt"), "12345").unwrap();
        std::fs::write(d.join("A.txt"), "").unwrap();
        let l = list_dir(&d).unwrap();
        let e = l["entries"].as_array().unwrap();
        assert_eq!(e.iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), vec!["Zeta", "A.txt", "b.txt"]);
        assert_eq!(e[0]["kind"], "dir");
        assert_eq!(e[2]["size"], 5);
        assert_eq!(l["path"], "~/docs");
        assert_eq!(l["parent"], "~");
        assert_eq!(list_dir(&dir.join("home")).unwrap()["parent"], Value::Null, "the home folder has no parent to show");
        assert!(list_dir(&d.join("b.txt")).is_err());
        for i in 0..(MAX_ENTRIES + 20) {
            std::fs::write(d.join(format!("f{i:04}")), "").unwrap();
        }
        let l = list_dir(&d).unwrap();
        assert_eq!(l["entries"].as_array().unwrap().len(), MAX_ENTRIES);
        assert_eq!(l["truncated"], true);
        restore(old, &dir);
    }

    #[test]
    fn reading_gives_text_only_and_caps_the_size() {
        let (_g, dir, old) = home_sandbox("read");
        let d = dir.join("home/docs");
        std::fs::write(d.join("t.txt"), "héllo\nworld").unwrap();
        let r = read_text(&d.join("t.txt")).unwrap();
        assert_eq!(r["text"], "héllo\nworld");
        assert_eq!(r["truncated"], false);
        std::fs::write(d.join("bin"), [1u8, 0, 2, 3]).unwrap();
        assert!(read_text(&d.join("bin")).unwrap_err().contains("binary"));
        std::fs::write(d.join("latin"), [0xE9u8, 0xE8, 0x41]).unwrap();
        assert!(read_text(&d.join("latin")).unwrap_err().contains("UTF-8"));
        assert!(read_text(&d).is_err(), "a folder is not a file");
        // a big file is cut, and a cut inside a character does not make it "not text"
        std::fs::write(d.join("big"), "é".repeat(MAX_READ_BYTES)).unwrap();
        let r = read_text(&d.join("big")).unwrap();
        assert_eq!(r["truncated"], true);
        assert!(r["text"].as_str().unwrap().len() <= MAX_READ_BYTES);
        let fifo = d.join("fifo");
        let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(resolve("docs/fifo").is_ok());
        assert!(read_text(&fifo).is_err(), "a pipe is not read (and does not block)");
        restore(old, &dir);
    }

    #[test]
    fn dialog_text_is_cleaned() {
        assert_eq!(clean("a\u{202e}b\nc\u{0}d", 10), "abcd");
        assert_eq!(clean("abcdef", 3), "abc");
    }

    #[test]
    fn every_control_op_that_shows_personal_content_asks_first() {
        assert!(!needs_approval("sys_info"));
        for op in ["fs_list", "fs_read", "screenshot"] {
            assert!(needs_approval(op), "{op}");
            assert!(OPS.contains(&op));
        }
        assert!(!needs_approval("unknown"));
    }
}
