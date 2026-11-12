//! Terminals that live inside ZO-AI: a shell on a pseudo-terminal, a screen kept up to date by a terminal emulator (vt100),
//! and a registry so the window (which draws and types into them) and the AI's tools (which open, type into and read
//! them) work on the very same terminals.

pub use vt100;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const SCROLLBACK: usize = 2000;

pub struct Term {
    pub id: u32,
    pub name: Mutex<String>,
    /// "the AI opened it" or "you opened it": shown in the interface.
    pub by_ai: bool,
    parser: Mutex<vt100::Parser>,
    master: Mutex<std::fs::File>,
    child: Mutex<Child>,
    /// Counts every change of the screen, so a window can tell when to redraw.
    pub version: AtomicU64,
    alive: AtomicBool,
}

static REGISTRY: OnceLock<Mutex<BTreeMap<u32, Arc<Term>>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
static ENABLED: AtomicBool = AtomicBool::new(false);

fn registry() -> &'static Mutex<BTreeMap<u32, Arc<Term>>> {
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// The terminal tools only exist inside the app (a terminal has to be drawn somewhere).
pub fn enable_in_app() {
    ENABLED.store(true, Ordering::SeqCst);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

pub fn list() -> Vec<Arc<Term>> {
    registry().lock().unwrap().values().cloned().collect()
}

pub fn get(id: u32) -> Option<Arc<Term>> {
    registry().lock().unwrap().get(&id).cloned()
}

fn winsize(rows: u16, cols: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

/// Opens a terminal running the user's shell (or `cmd` through it) in `cwd`.
pub fn open(name: &str, cwd: Option<&str>, cmd: Option<&str>, by_ai: bool) -> Result<Arc<Term>, String> {
    let (rows, cols) = (30u16, 110u16);
    let (mut m, mut s) = (0, 0);
    let ws = winsize(rows, cols);
    // SAFETY: openpty fills the two descriptors we pass; they are wrapped right away.
    if unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) } != 0 {
        return Err("Could not open a terminal (no pseudo-terminal available).".into());
    }
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) };
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/bash".into());
    let mut c = Command::new(&shell);
    crate::exec::scrub(&mut c);
    if let Some(cmd) = cmd {
        c.args(["-c", cmd]);
    }
    if let Some(d) = cwd.filter(|d| !d.is_empty()) {
        c.current_dir(d);
    }
    c.env("TERM", "xterm-256color").env("COLORTERM", "truecolor");
    let sfd = slave.as_raw_fd();
    c.stdin(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
        .stdout(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(slave));
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        c.pre_exec(move || {
            libc::setsid();
            libc::ioctl(sfd, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let child = c.spawn().map_err(|e| format!("Could not start {shell}: {e}"))?;
    let reader = std::fs::File::from(master.try_clone().map_err(|e| e.to_string())?);
    let writer = std::fs::File::from(master);
    let id = NEXT.fetch_add(1, Ordering::SeqCst) as u32;
    let term = Arc::new(Term {
        id,
        name: Mutex::new(if name.is_empty() { format!("Terminal {id}") } else { name.to_string() }),
        by_ai,
        parser: Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)),
        master: Mutex::new(writer),
        child: Mutex::new(child),
        version: AtomicU64::new(1),
        alive: AtomicBool::new(true),
    });
    let t2 = term.clone();
    std::thread::spawn(move || {
        let mut r = reader;
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    t2.parser.lock().unwrap().process(&buf[..n]);
                    t2.version.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        t2.alive.store(false, Ordering::SeqCst);
        t2.version.fetch_add(1, Ordering::SeqCst);
    });
    registry().lock().unwrap().insert(id, term.clone());
    Ok(term)
}

impl Term {
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn name(&self) -> String {
        self.name.lock().unwrap().clone()
    }

    /// Types bytes into the terminal, as if from the keyboard.
    pub fn write(&self, data: &[u8]) {
        let mut m = self.master.lock().unwrap();
        let _ = m.write_all(data);
        let _ = m.flush();
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(2), cols.max(10));
        {
            let mut p = self.parser.lock().unwrap();
            if p.screen().size() == (rows, cols) {
                return;
            }
            p.set_size(rows, cols);
        }
        let ws = winsize(rows, cols);
        let fd = self.master.lock().unwrap().as_raw_fd();
        // SAFETY: fd is the open master; ws is a valid winsize.
        unsafe { libc::ioctl(fd, libc::TIOCSWINSZ as _, &ws) };
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    /// Runs `f` with the emulator's screen, scrolled back by `back` lines.
    pub fn with_screen<R>(&self, back: usize, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        let mut p = self.parser.lock().unwrap();
        p.set_scrollback(back);
        let r = f(p.screen());
        p.set_scrollback(0);
        r
    }

    /// The text on the screen now (what the AI reads).
    pub fn screen_text(&self) -> String {
        let t = self.with_screen(0, |s| s.contents());
        let lines: Vec<&str> = t.lines().collect();
        lines.join("\n").trim_end().to_string()
    }

    /// The last `n` lines, including what scrolled off the screen.
    pub fn tail(&self, n: usize) -> String {
        let mut p = self.parser.lock().unwrap();
        let (rows, _) = p.screen().size();
        let mut all: Vec<String> = Vec::new();
        // the scrollback is read a screenful at a time, oldest first
        let mut back = SCROLLBACK;
        p.set_scrollback(usize::MAX);
        let total = p.screen().scrollback();
        back = back.min(total);
        let mut off = back;
        while off > 0 {
            p.set_scrollback(off);
            let text = p.screen().contents();
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            let take = (off.min(rows as usize)).min(lines.len());
            all.extend(lines.into_iter().take(take));
            off = off.saturating_sub(rows as usize);
        }
        p.set_scrollback(0);
        let text = p.screen().contents();
        all.extend(text.lines().map(str::to_string));
        while all.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            all.pop();
        }
        let start = all.len().saturating_sub(n);
        all[start..].join("\n")
    }

    pub fn close(&self) {
        let _ = self.child.lock().unwrap().kill();
        let _ = self.child.lock().unwrap().wait();
        registry().lock().unwrap().remove(&self.id);
        self.alive.store(false, Ordering::SeqCst);
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    /// Types a command and waits until the output stops changing (or `wait` has passed), then returns the screen.
    pub fn run(&self, command: &str, wait: Duration) -> String {
        let before = self.version.load(Ordering::SeqCst);
        self.write(format!("{command}\r").as_bytes());
        let start = Instant::now();
        let mut last = before;
        let mut quiet_since = Instant::now();
        while start.elapsed() < wait {
            std::thread::sleep(Duration::from_millis(100));
            let v = self.version.load(Ordering::SeqCst);
            if v != last {
                last = v;
                quiet_since = Instant::now();
            } else if v != before && quiet_since.elapsed() > Duration::from_millis(1200) {
                break;
            }
        }
        self.tail(60)
    }
}

/// Bytes for a key by name, for programs that need Enter, arrows, Ctrl+C and so on.
pub fn key_bytes(name: &str) -> Option<Vec<u8>> {
    let n = name.trim().to_lowercase();
    let n = n.replace("control", "ctrl");
    if let Some(c) = n.strip_prefix("ctrl+").or_else(|| n.strip_prefix("ctrl-")) {
        let ch = c.chars().next()?;
        if c.chars().count() == 1 && ch.is_ascii_alphabetic() {
            return Some(vec![(ch as u8) & 0x1f]);
        }
        return None;
    }
    Some(match n.as_str() {
        "enter" | "return" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" | "esc" => vec![0x1b],
        "backspace" => vec![0x7f],
        "delete" => b"\x1b[3~".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "space" => b" ".to_vec(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_runs_and_its_output_is_read_back() {
        let t = open("test", None, None, true).unwrap();
        let out = t.run("echo zo-term-$((6*7))", Duration::from_secs(8));
        t.close();
        assert!(out.contains("zo-term-42"), "{out}");
    }

    #[test]
    fn keys_have_bytes() {
        assert_eq!(key_bytes("Ctrl+C"), Some(vec![3]));
        assert_eq!(key_bytes("Enter"), Some(b"\r".to_vec()));
        assert_eq!(key_bytes("nonsense"), None);
    }

    #[test]
    fn the_screen_follows_colour_codes_and_cursor_moves() {
        let t = open("test", None, Some("printf '\\033[31mred\\033[0m ok'; sleep 1"), true).unwrap();
        std::thread::sleep(Duration::from_millis(700));
        let s = t.screen_text();
        t.close();
        assert!(s.contains("red ok"), "{s}");
    }
}
