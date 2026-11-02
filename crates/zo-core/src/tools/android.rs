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

fn is_root() -> bool {
    std::env::var("USER").map(|u| u == "root").unwrap_or(false)
}
