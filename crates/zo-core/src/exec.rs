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

pub fn run(cmd: &mut Command, limit: Duration) -> Result<Out, String> {
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
