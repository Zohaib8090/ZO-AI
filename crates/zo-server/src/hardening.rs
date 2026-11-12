//! The server's secrets are read once at the very start and then taken out of its own environment, so that no program it starts (a
//! tool, Claude Code) and no `/proc/<pid>/environ` can show them. `remove_var` alone is not enough: Linux keeps showing the
//! environment the process was started with (the strings on its stack), so those bytes are overwritten first.
//!
//! The values live on in memory only: the Firebase key in the Firestore client, the secret in zo-core's key store
//! (`zo_core::ai::keys::set_server_secret`), the global Claude token in `zo_core::ai::claude_code::set_global_token`.

use std::ffi::CStr;

/// The variables that are secrets.
pub const SECRET_VARS: [&str; 3] = ["FIREBASE_SERVICE_ACCOUNT", "ZO_SERVER_SECRET", "CLAUDE_CODE_OAUTH_TOKEN"];

#[derive(Default)]
pub struct Secrets {
    pub firebase: Option<String>,
    pub server_secret: Option<String>,
    pub claude_token: Option<String>,
}

extern "C" {
    static environ: *const *mut std::os::raw::c_char;
}

/// Overwrites the value of `name` in every entry of the process environment block with zeros (the strings that `/proc/<pid>/environ`
/// shows), and then removes the variable.
fn wipe(name: &str) {
    let prefix = format!("{name}=");
    // SAFETY: `environ` is the C library's table of `NAME=value` strings, each NUL-terminated and writable (the initial ones sit in
    // the process's own stack block, the others were made by setenv). This runs at the start of main, before any other thread, and
    // only zeroes the bytes after the `=` of the entries that carry this name; it does not move or free anything. The table is
    // not changed while it is walked.
    unsafe {
        let mut p = environ;
        if !p.is_null() {
            while !(*p).is_null() {
                let entry = *p;
                let len = CStr::from_ptr(entry).to_bytes().len();
                if CStr::from_ptr(entry).to_bytes().starts_with(prefix.as_bytes()) {
                    std::ptr::write_bytes(entry.add(prefix.len()), 0, len - prefix.len());
                }
                p = p.add(1);
            }
        }
    }
    std::env::remove_var(name);
}

fn take(name: &str) -> Option<String> {
    let v = std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    wipe(name);
    v
}

/// The secrets as the Docker entrypoint (`zo-entrypoint.sh`) hands them over: lines `NAME=base64(value)` on the file descriptor named
/// by `ZO_SECRETS_FD`. They never sit in the environment of tini (process 1, the same user as this server), whose
/// `/proc/1/environ` an admin's shell could otherwise read. Returns the decoded values; empty when there is no such descriptor.
fn from_fd() -> std::collections::HashMap<String, String> {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let fd = std::env::var("ZO_SECRETS_FD").ok().and_then(|v| v.trim().parse::<i32>().ok()).filter(|n| *n >= 3);
    wipe("ZO_SECRETS_FD");
    let Some(fd) = fd else { return Default::default() };
    // SAFETY: the entrypoint opened this descriptor for us and nothing else in this process uses it; it is closed when `f` is dropped.
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut text = String::new();
    let _ = f.read_to_string(&mut text);
    parse_fd_text(&text)
}

fn parse_fd_text(text: &str) -> std::collections::HashMap<String, String> {
    use base64::Engine;
    text.lines()
        .filter_map(|l| {
            let (name, b64) = l.split_once('=')?;
            let v = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
            let v = String::from_utf8(v).ok()?.trim().to_string();
            (!v.is_empty()).then(|| (name.trim().to_string(), v))
        })
        .collect()
}

/// Reads the three secret variables (from the entrypoint's descriptor, else from the environment) and removes them from the
/// environment. Call it first thing in `main`.
pub fn take_secrets() -> Secrets {
    let mut fd = from_fd();
    let mut get = |name: &str| {
        let env = take(name);
        fd.remove(name).or(env)
    };
    Secrets { firebase: get(SECRET_VARS[0]), server_secret: get(SECRET_VARS[1]), claude_token: get(SECRET_VARS[2]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREBASE: &str = "{\"private_key\":\"firebase-secret-value-1234567890\"}";
    const SECRET: &str = "server-secret-value-abcdefghij";
    const TOKEN: &str = "sk-ant-oat01-global-token-value-0123456789";

    /// Runs in a process of its own (see the driver below): reads its own environment the way an attacker would.
    #[test]
    fn probe() {
        let Ok(out) = std::env::var("ZO_HARDEN_PROBE_OUT") else { return };
        let before = std::env::var("FIREBASE_SERVICE_ACCOUNT").is_ok();
        let s = take_secrets();
        let mut report = format!("before={before}\ngot={}|{}|{}\n", s.firebase.as_deref() == Some(FIREBASE), s.server_secret.as_deref() == Some(SECRET), s.claude_token.as_deref() == Some(TOKEN));
        report += &format!("after={}\n", SECRET_VARS.iter().any(|n| std::env::var(n).is_ok()));
        // a child program: its environment, and the environment block of this process as /proc shows it to the child
        let child = std::process::Command::new("sh").args(["-c", "env; echo ===PROC===; tr '\\0' '\\n' < /proc/$PPID/environ"]).output().unwrap();
        report += &String::from_utf8_lossy(&child.stdout);
        // and this process's own view of it
        report += "\n===SELF===\n";
        report += &String::from_utf8_lossy(&std::fs::read("/proc/self/environ").unwrap()).replace('\0', "\n");
        std::fs::write(out, report).unwrap();
    }

    #[test]
    fn the_entrypoints_lines_are_decoded() {
        use base64::Engine;
        let b = |v: &str| base64::engine::general_purpose::STANDARD.encode(v);
        let text = format!("FIREBASE_SERVICE_ACCOUNT={}\nZO_SERVER_SECRET={}\nCLAUDE_CODE_OAUTH_TOKEN=\n", b("{\n \"a\": 1\n}"), b("s3cret"));
        let m = parse_fd_text(&text);
        assert_eq!(m["FIREBASE_SERVICE_ACCOUNT"], "{\n \"a\": 1\n}");
        assert_eq!(m["ZO_SERVER_SECRET"], "s3cret");
        assert!(!m.contains_key("CLAUDE_CODE_OAUTH_TOKEN"), "an empty value is no value");
    }

    #[test]
    fn the_secrets_are_gone_from_the_environment_the_children_and_proc_environ_but_the_server_got_them() {
        let dir = std::env::temp_dir().join(format!("zo-harden-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("report");
        let exe = std::env::current_exe().unwrap();
        let st = std::process::Command::new(exe)
            .args(["--exact", "hardening::tests::probe", "--test-threads=1", "--nocapture"])
            .env("ZO_HARDEN_PROBE_OUT", &out)
            .env("FIREBASE_SERVICE_ACCOUNT", FIREBASE)
            .env("ZO_SERVER_SECRET", SECRET)
            .env("CLAUDE_CODE_OAUTH_TOKEN", TOKEN)
            .env("ZO_ADMIN_EMAILS", "owner@example.test")
            .output()
            .unwrap();
        assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stdout));
        let report = std::fs::read_to_string(&out).expect("the probe wrote its report");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(report.starts_with("before=true\ngot=true|true|true\nafter=false\n"), "{report}");
        for secret in ["firebase-secret-value", SECRET, TOKEN, "private_key"] {
            assert!(!report.contains(secret), "{secret} is still visible:\n{report}");
        }
        // the variables that are not secrets are untouched
        let (proc_part, _) = report.split_once("===SELF===").unwrap();
        assert!(proc_part.contains("ZO_ADMIN_EMAILS=owner@example.test"), "{proc_part}");
        assert!(proc_part.contains("===PROC==="));
    }
}
