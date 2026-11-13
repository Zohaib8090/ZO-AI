//! The old command name. The product is now called Zohara Chat and its program is `zohara-chat`;
//! `zo-ai` only prints a hint and starts the new program with the same arguments, so old launchers,
//! scripts and autostart entries keep working.

use std::os::unix::process::CommandExt;

/// The program to start: the `zohara-chat` next to this one, else the one on PATH.
fn target() -> std::path::PathBuf {
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            let sibling = dir.join("zohara-chat");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    "zohara-chat".into()
}

fn main() {
    eprintln!("zo-ai: this command is now called `zohara-chat` (ZO-AI became Zohara Chat); starting it for you.");
    let err = std::process::Command::new(target()).args(std::env::args_os().skip(1)).exec();
    eprintln!("zo-ai: could not start zohara-chat: {err}");
    std::process::exit(127);
}
