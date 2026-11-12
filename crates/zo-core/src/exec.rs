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
const SERVER_ENV: &[&str] = &["FIREBASE_SERVICE_ACCOUNT", "ZO_SERVER_SECRET", "ZO_ACCESS_TOKEN"];

/// Takes every secret out of what a child program inherits: the server's variables, the Claude Code login token
/// and the provider keys (`ZOAI_KEY_*`).
pub fn scrub(cmd: &mut Command) -> &mut Command {
    scrub_server(cmd);
    cmd.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("ZOAI_KEY_") {
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
    scrub(cmd);
    let mut child = cmd
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
            None if start.elapsed() >= limit => {
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

/// Shorthand: program + args, 30 s limit.
pub fn simple(program: &str, args: &[&str]) -> Result<String, String> {
    run(Command::new(program).args(args), Duration::from_secs(30)).map(|o| o.text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_do_not_get_the_servers_secrets() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo \"[$FIREBASE_SERVICE_ACCOUNT|$ZO_SERVER_SECRET|$ZO_ACCESS_TOKEN|$CLAUDE_CODE_OAUTH_TOKEN|$HOME_OK]\""])
            .env("FIREBASE_SERVICE_ACCOUNT", "a")
            .env("ZO_SERVER_SECRET", "b")
            .env("ZO_ACCESS_TOKEN", "c")
            .env("CLAUDE_CODE_OAUTH_TOKEN", "d")
            .env("HOME_OK", "e");
        assert_eq!(run(&mut c, Duration::from_secs(5)).unwrap().text(), "[||||e]");
    }
}
