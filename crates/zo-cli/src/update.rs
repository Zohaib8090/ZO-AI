//! `zo version` and `zo update-check` (docs/UPDATES.md). The check asks the Zohara Agent server (never GitHub) and only prints: it never
//! downloads or installs anything. Exit codes: 0 when the check worked (up to date or not), 1 when it could not be done.

use zo_core::update::{Status, Verdict};

pub fn cmd_version() -> i32 {
    println!("zo {}", zo_core::update::VERSION);
    0
}

pub fn cmd_update_check(server: Result<String, String>) -> i32 {
    let base = match server {
        Ok(b) => b,
        Err(e) => {
            eprintln!("zo: {e}");
            return 1;
        }
    };
    match zo_core::update::check_server(&base, zo_core::update::VERSION) {
        Ok(st) => {
            if st.verdict == Verdict::Unknown {
                eprintln!("zo: {}", render(&st));
                return 1;
            }
            println!("{}", render(&st));
            0
        }
        Err(e) => {
            eprintln!("zo: could not check for updates: {e}");
            1
        }
    }
}

/// The text printed for a status.
pub fn render(st: &Status) -> String {
    let mut out = st.summary();
    if let (Verdict::Available, Some(l)) = (st.verdict, &st.latest) {
        if st.required {
            out.push_str("\nThis version is no longer supported: please update.");
        }
        if !l.date.is_empty() {
            out.push_str(&format!("\nReleased {}", l.date));
        }
        if !l.notes.is_empty() {
            out.push_str(&format!("\n\nWhat is new:\n{}", l.notes));
        }
        out.push_str(&format!("\n\nRelease page: {}", l.release_url));
        match (&l.assets.pkg, zo_core::update::pacman_command(l)) {
            (Some(pkg), Some(cmd)) => out.push_str(&format!("\nOn Arch Linux: download {pkg} from that page, then run: {cmd}")),
            _ => out.push_str("\nOn Zohara OS the update also arrives through Settings > Updates."),
        }
        out.push_str("\nzo never installs updates by itself.");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(current: &str, v: serde_json::Value) -> Status {
        Status::from_api(current, &json!({"ok": true, "repo": "o/r", "server_version": "0.2.0", "latest": v}))
    }

    #[test]
    fn an_available_update_shows_notes_page_and_the_pacman_command() {
        let st = status("0.1.0", json!({"version": "0.2.0", "date": "2026-11-20", "notes": "Update checks", "assets": {"pkg": "zohara-agent-0.2.0-x86_64.pkg.tar.zst"}}));
        let t = render(&st);
        assert!(t.starts_with("Version 0.2.0 is available (you have 0.1.0)."), "{t}");
        assert!(t.contains("Update checks") && t.contains("https://github.com/o/r/releases/tag/v0.2.0"));
        assert!(t.contains("sudo pacman -U zohara-agent-0.2.0-x86_64.pkg.tar.zst"));
        assert!(t.contains("never installs"));
    }

    #[test]
    fn up_to_date_and_unknown_are_one_line() {
        assert_eq!(render(&status("0.2.0", json!({"version": "0.2.0"}))), "You are up to date (0.2.0).");
        let none = Status::from_api("0.1.0", &json!({"ok": true, "latest": null, "error": "no release has been published yet"}));
        assert_eq!(render(&none), "Could not check for updates: no release has been published yet.");
    }

    #[test]
    fn unsupported_versions_are_told_to_update() {
        let st = status("0.1.0", json!({"version": "0.3.0", "min_supported": "0.2.0"}));
        assert!(render(&st).contains("no longer supported"));
    }

    #[test]
    fn exit_codes_follow_whether_the_check_worked() {
        assert_eq!(cmd_update_check(Err("bad address".into())), 1);
        assert_eq!(cmd_update_check(Ok("http://127.0.0.1:9".into())), 1);
        assert_eq!(cmd_version(), 0);
    }
}
