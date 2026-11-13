//! The old command name `zohara-chat`. The product is now called Zohara Agent and its program is `zohara-agent` (Zohara Chat became Zohara Agent);
//! `zohara-chat` only prints a hint and starts the new program with the same arguments, so old launchers,
//! scripts and autostart entries keep working.

use std::os::unix::process::CommandExt;

/// The program to start: the `zohara-agent` next to this one, else the one on PATH.
fn target() -> std::path::PathBuf {
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            let sibling = dir.join("zohara-agent");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    "zohara-agent".into()
}

fn main() {
    eprintln!("zohara-chat: this command is now called `zohara-agent` (Zohara Chat became Zohara Agent); starting it for you.");
    let err = std::process::Command::new(target()).args(std::env::args_os().skip(1)).exec();
    eprintln!("zohara-chat: could not start zohara-agent: {err}");
    std::process::exit(127);
}
