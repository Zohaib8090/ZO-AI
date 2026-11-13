//! Run a program with a time limit and capped output.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_OUTPUT: usize = 100_000;

pub struct Out {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

impl Out {
    /// stdout if there is any, else stderr, with the exit code appended when it is not 0.
    pub fn text(&self) -> String {
        let body = if !self.stdout.trim().is_empty() {
            self.stdout.trim()
        } else if !self.stderr.trim().is_empty() {
            self.stderr.trim()
        } else {
            "Command completed (no output)"
        };
        match self.code {
            Some(0) => body.to_string(),
            Some(c) => format!("{body}\n[exit code {c}]"),
            None => format!("{body}\n[stopped by a signal]"),
        }
    }
}

fn drain(mut r: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        while let Ok(n) = r.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if buf.len() < MAX_OUTPUT {
                buf.extend_from_slice(&chunk[..n.min(MAX_OUTPUT - buf.len())]);
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// Variables that hold the server's own secrets. No program the AI starts gets them.
const SERVER_ENV: &[&str] = &["FIREBASE_SERVICE_ACCOUNT", "ZO_SERVER_SECRET", "ZO_TENANT_ROOT"];

/// Takes every secret out of what a child program inherits: the server's variables, the Claude Code login token
/// and the provider keys (`ZOHARA_AGENT_KEY_*`).
pub fn scrub(cmd: &mut Command) -> &mut Command {
    scrub_server(cmd);
    cmd.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
    for (k, _) in std::env::vars_os() {
        // the current prefix and the ones used before the renames
        let name = k.to_string_lossy();
        if name.starts_with("ZOAI_KEY_") || crate::legacy::ENV_PREFIXES.iter().any(|p| name.strip_prefix(p).is_some_and(|r| r.starts_with("KEY_"))) {
            cmd.env_remove(k);
        }
    }
    cmd
}

/// Only the server's own variables: for Claude Code itself, which needs its login token and the provider keys.
pub fn scrub_server(cmd: &mut Command) -> &mut Command {
    for k in SERVER_ENV {
        cmd.env_remove(k);
    }
    cmd
}

pub fn run(cmd: &mut Command, limit: Duration) -> Result<Out, String> {
    run_cancellable(cmd, limit, &std::sync::atomic::AtomicBool::new(false))
}

/// Like `run`, and the program is killed as soon as `cancel` is set (the Stop button of the activity list).
pub fn run_cancellable(cmd: &mut Command, limit: Duration, cancel: &std::sync::atomic::AtomicBool) -> Result<Out, String> {
    run_until(cmd, limit, &|| cancel.load(std::sync::atomic::Ordering::Relaxed))
}

/// Like `run_cancellable`, with any "should stop" test (for example two flags). The program runs in a group of its own, and Stop or
/// the time limit kills the whole group, so what a shell started (`make && ./deploy`) does not go on in the background.
pub fn run_until(cmd: &mut Command, limit: Duration, stop: &dyn Fn() -> bool) -> Result<Out, String> {
    use std::os::unix::process::CommandExt;
    scrub(cmd);
    let mut child = cmd
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start: {e}"))?;
    let out = drain(child.stdout.take().ok_or("no stdout")?);
    let err = drain(child.stderr.take().ok_or("no stderr")?);

    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if stop() => {
                kill_group(&child);
                let _ = child.kill();
                let _ = child.wait();
                return Err("Stopped.".into());
            }
            None if start.elapsed() >= limit => {
                kill_group(&child);
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} s", limit.as_secs()));
            }
            None => thread::sleep(Duration::from_millis(15)),
        }
    };
    Ok(Out {
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
        code: status.code(),
    })
}

fn kill_group(child: &std::process::Child) {
    // the child leads a group of its own (process_group(0)), so its pid is the group id
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
}

/// Shorthand: program + args, 30 s limit.
pub fn simple(program: &str, args: &[&str]) -> Result<String, String> {
    run(Command::new(program).args(args), Duration::from_secs(30)).map(|o| o.text())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Provider keys under the current prefix and under every prefix used before the renames never reach a program the AI starts.
    #[test]
    fn scrub_removes_keys_of_all_prefixes() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let names = ["ZOHARA_AGENT_KEY_SCRUBTEST", "ZOHARA_CHAT_KEY_SCRUBTEST", "ZOHARA_AI_KEY_SCRUBTEST", "ZO_AI_KEY_SCRUBTEST", "ZOAI_KEY_SCRUBTEST"];
        for (i, n) in names.iter().enumerate() {
            std::env::set_var(n, format!("secret-number-{i}"));
        }
        let mut c = Command::new("sh");
        c.args(["-c", "env"]);
        scrub(&mut c);
        let out = c.output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        for n in names {
            std::env::remove_var(n);
        }
        assert!(!text.contains("secret-number-"), "{text}");
    }

    #[test]
    fn a_command_is_killed_when_stop_is_pressed() {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c2.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let t0 = Instant::now();
        let e = run_cancellable(Command::new("sleep").arg("20"), Duration::from_secs(30), &cancel).err().unwrap();
        assert_eq!(e, "Stopped.");
        assert!(t0.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn stop_kills_what_the_shell_started_too() {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            c2.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let marker = format!("zo-stop-test-{}", std::process::id());
        let script = format!("sleep 31 && echo {marker}; sleep 32; echo {marker}");
        let t0 = Instant::now();
        let e = run_cancellable(Command::new("sh").arg("-c").arg(&script), Duration::from_secs(30), &cancel).err().unwrap();
        assert_eq!(e, "Stopped.");
        assert!(t0.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(200));
        // no `sleep 31` or `sleep 32` of ours is left behind
        let left = Command::new("sh").arg("-c").arg("ps -eo args | grep -E '^sleep 3[12]$' | wc -l").output().unwrap();
        let n: usize = String::from_utf8_lossy(&left.stdout).trim().parse().unwrap_or(0);
        assert_eq!(n, 0, "children of the stopped shell are still running");
    }

    #[test]
    fn programs_do_not_get_the_servers_secrets() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo \"[$FIREBASE_SERVICE_ACCOUNT|$ZO_SERVER_SECRET|$ZO_TENANT_ROOT|$CLAUDE_CODE_OAUTH_TOKEN|$HOME_OK]\""])
            .env("FIREBASE_SERVICE_ACCOUNT", "a")
            .env("ZO_SERVER_SECRET", "b")
            .env("ZO_TENANT_ROOT", "c")
            .env("CLAUDE_CODE_OAUTH_TOKEN", "d")
            .env("HOME_OK", "e");
        assert_eq!(run(&mut c, Duration::from_secs(5)).unwrap().text(), "[||||e]");
    }
}
