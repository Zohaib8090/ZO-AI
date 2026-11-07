use crate::exec;
use std::process::Command;
use std::time::Duration;

fn adb_bin() -> String {
    std::env::var("ADB_BIN")
        .or_else(|_| std::env::var("ADB"))
        .unwrap_or_else(|_| if std::path::Path::new("/usr/sbin/adb").exists() { "/usr/sbin/adb".into() } else { "adb".into() })
}

pub fn adb(args: &[&str]) -> Result<String, String> {
    exec::run(Command::new(adb_bin()).args(args), Duration::from_secs(120)).map(|o| o.text())
}

pub fn adb_shell(cmds: &str) -> Result<String, String> {
    let mut args = vec!["shell"];
    args.extend(cmds.split_whitespace());
    adb(&args)
}

/// Waydroid's app, shell and logcat commands need root; `pkexec` asks the person for their password in a normal dialog.
pub fn waydroid(args: &[&str], root: bool) -> Result<String, String> {
    let mut cmd = if root && !is_root() {
        let mut c = Command::new("pkexec");
        c.arg("waydroid");
        c
    } else {
        Command::new("waydroid")
    };
    cmd.arg("--details-to-stdout").args(args);
    exec::run(&mut cmd, Duration::from_secs(120)).map(|o| o.text())
}

/// Is the Android session running? (`waydroid status` says "Session: RUNNING".)
fn waydroid_running() -> bool {
    waydroid(&["status"], false).map(|t| t.lines().any(|l| l.starts_with("Session:") && l.contains("RUNNING"))).unwrap_or(false)
}

/// Starts the Android session when it is stopped, and shows its window; waits until it runs (up to about 40 seconds).
pub fn waydroid_start() -> Result<String, String> {
    if waydroid_running() {
        return Ok("Waydroid is already running.".into());
    }
    // `show-full-ui` starts the session when needed and keeps running while the window is open, so it is left to run on its own
    Command::new("waydroid")
        .arg("show-full-ui")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start Waydroid: {e}"))?;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_secs(2));
        if waydroid_running() {
            return Ok("Waydroid is running now. Its window is open, and its apps can be listed.".into());
        }
    }
    Err("Waydroid was asked to start but is not running yet. Give it a little longer, then check its status.".into())
}

fn is_root() -> bool {
    std::env::var("USER").map(|u| u == "root").unwrap_or(false)
}
