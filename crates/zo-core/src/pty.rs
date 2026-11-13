//! A program on a pseudo-terminal whose output is kept as RAW bytes in a bounded ring with running sequence numbers. This is what a
//! terminal in a browser (xterm.js) or a login screen needs: the screen is drawn by the client, so there is no emulator here, and a
//! client that lost its connection asks for "everything after byte N" and gets exactly that (or is told what it missed).
//!
//! The terminals of `term.rs` (the AI's own, drawn by the desktop window) keep a parsed screen instead and are not changed.
//!
//! Safety properties, all tested:
//! * the descriptors of the pseudo-terminal are close-on-exec, so a later program (of another person's session) can never inherit
//!   the master of an earlier one;
//! * `clear_env` starts the program with exactly the environment given and nothing of ours;
//! * the program leads a session and a process group of its own: closing the terminal kills the whole group;
//! * optional resource limits (CPU time, file size, open files, processes, data memory, no core files) are set between fork and exec.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How much output is kept for a client that reconnects.
pub const RING_BYTES: usize = 1024 * 1024;
/// The most input one write may carry.
pub const MAX_INPUT: usize = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub struct Limits {
    /// CPU seconds the program may use in total.
    pub cpu_secs: Option<u64>,
    /// The biggest file it may write, in bytes.
    pub fsize: Option<u64>,
    pub nofile: Option<u64>,
    /// Processes of this user (counts every process and thread of the same user, the server's too: keep it generous).
    pub nproc: Option<u64>,
    /// Writable memory (heap, stacks, private mappings), in bytes.
    pub data: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// When true the program gets only `env`; otherwise it inherits ours plus `env`.
    pub clear_env: bool,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub rows: u16,
    pub cols: u16,
    pub limits: Option<Limits>,
}

impl Spec {
    pub fn new(program: impl Into<PathBuf>) -> Spec {
        Spec { program: program.into(), args: Vec::new(), clear_env: false, env: Vec::new(), cwd: None, rows: 30, cols: 110, limits: None }
    }
}

struct Ring {
    buf: VecDeque<u8>,
    /// Sequence number of the first byte in `buf` (bytes before it were dropped).
    base: u64,
}

pub struct Pty {
    child: Mutex<Child>,
    pid: i32,
    writer: Mutex<std::fs::File>,
    ring: Mutex<Ring>,
    more: Condvar,
    alive: AtomicBool,
    code: AtomicI32,
    started: Instant,
    /// Milliseconds since `started` of the last input or output.
    active: AtomicU64,
    size: Mutex<(u16, u16)>,
}

fn winsize(rows: u16, cols: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

fn set_cloexec(fd: i32) {
    // SAFETY: plain fcntl calls on a descriptor we own.
    unsafe {
        let f = libc::fcntl(fd, libc::F_GETFD);
        if f >= 0 {
            libc::fcntl(fd, libc::F_SETFD, f | libc::FD_CLOEXEC);
        }
    }
}

/// Kills every process of the session that `sid` leads. An interactive shell puts each job in a process group of its own, so killing
/// the shell's group is not enough: everything it started shares its SESSION. (A program that starts a session of its own
/// escapes; without cgroups nothing here can follow it.)
fn kill_session(sid: i32) {
    let Ok(rd) = std::fs::read_dir("/proc") else { return };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
        // "pid (comm) state ppid pgrp session ...": the name may hold spaces and parentheses, so count from the last ')'
        let Some(rest) = stat.rfind(')').map(|i| &stat[i + 1..]) else { continue };
        let f: Vec<&str> = rest.split_whitespace().collect();
        if f.get(3).and_then(|s| s.parse::<i32>().ok()) == Some(sid) {
            // SAFETY: a signal to a process of the session this terminal led.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

/// Result of `read_from`.
#[derive(Debug, PartialEq, Eq)]
pub struct Chunk {
    /// Sequence number of the first byte of `data`. Bigger than the one asked for when output was dropped meanwhile.
    pub start: u64,
    pub data: Vec<u8>,
    /// The sequence number to ask for next.
    pub next: u64,
    /// The program has ended and everything it wrote has been given out.
    pub ended: bool,
}

impl Pty {
    pub fn spawn(spec: &Spec) -> Result<Arc<Pty>, String> {
        let (rows, cols) = (spec.rows.clamp(2, 500), spec.cols.clamp(10, 2000));
        let (mut m, mut s) = (0, 0);
        let ws = winsize(rows, cols);
        // SAFETY: openpty fills the two descriptors we pass; they are wrapped right away.
        if unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) } != 0 {
            return Err("Could not open a terminal (no pseudo-terminal available).".into());
        }
        set_cloexec(m);
        set_cloexec(s);
        // SAFETY: both descriptors are open and owned by nobody else.
        let (master, slave) = unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) };
        let mut c = Command::new(&spec.program);
        c.args(&spec.args);
        if spec.clear_env {
            c.env_clear();
        }
        for (k, v) in &spec.env {
            c.env(k, v);
        }
        if let Some(d) = &spec.cwd {
            c.current_dir(d);
        }
        let sfd = slave.as_raw_fd();
        c.stdin(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stdout(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(slave));
        let limits = spec.limits.clone();
        // SAFETY: only async-signal-safe calls (setsid, ioctl, setrlimit) between fork and exec.
        unsafe {
            c.pre_exec(move || {
                libc::setsid();
                libc::ioctl(sfd, libc::TIOCSCTTY as _, 0);
                // a program started by a parent that ignores or blocks signals (a background job of a shell, a daemon) would pass that on:
                // Ctrl-C must work in a terminal
                for sig in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP, libc::SIGPIPE, libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU, libc::SIGALRM, libc::SIGUSR1, libc::SIGUSR2, libc::SIGCHLD] {
                    libc::signal(sig, libc::SIG_DFL);
                }
                let mut none: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut none);
                libc::sigprocmask(libc::SIG_SETMASK, &none, std::ptr::null_mut());
                let set = |res: libc::__rlimit_resource_t, v: u64| {
                    let r = libc::rlimit { rlim_cur: v as libc::rlim_t, rlim_max: v as libc::rlim_t };
                    libc::setrlimit(res, &r);
                };
                set(libc::RLIMIT_CORE, 0);
                if let Some(l) = &limits {
                    if let Some(v) = l.cpu_secs {
                        set(libc::RLIMIT_CPU, v);
                    }
                    if let Some(v) = l.fsize {
                        set(libc::RLIMIT_FSIZE, v);
                    }
                    if let Some(v) = l.nofile {
                        set(libc::RLIMIT_NOFILE, v);
                    }
                    if let Some(v) = l.nproc {
                        set(libc::RLIMIT_NPROC, v);
                    }
                    if let Some(v) = l.data {
                        set(libc::RLIMIT_DATA, v);
                    }
                }
                Ok(())
            });
        }
        let child = c.spawn().map_err(|e| format!("Could not start {}: {e}", spec.program.display()))?;
        let pid = child.id() as i32;
        let reader = std::fs::File::from(master.try_clone().map_err(|e| e.to_string())?);
        let writer = std::fs::File::from(master);
        let p = Arc::new(Pty {
            child: Mutex::new(child),
            pid,
            writer: Mutex::new(writer),
            ring: Mutex::new(Ring { buf: VecDeque::new(), base: 0 }),
            more: Condvar::new(),
            alive: AtomicBool::new(true),
            code: AtomicI32::new(-1),
            started: Instant::now(),
            active: AtomicU64::new(0),
            size: Mutex::new((rows, cols)),
        });
        // the output
        let p2 = p.clone();
        std::thread::spawn(move || {
            let mut r = reader;
            let mut buf = [0u8; 8192];
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut g = p2.ring.lock().unwrap_or_else(|e| e.into_inner());
                        g.buf.extend(&buf[..n]);
                        let over = g.buf.len().saturating_sub(RING_BYTES);
                        if over > 0 {
                            g.buf.drain(..over);
                            g.base += over as u64;
                        }
                        drop(g);
                        p2.touch();
                        p2.more.notify_all();
                    }
                }
            }
            // the terminal is closed on the program's side: it is about to end (the reaper stores the exit code); wait a moment for that
            let t0 = Instant::now();
            while p2.code.load(Ordering::SeqCst) < 0 && t0.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(20));
            }
            p2.alive.store(false, Ordering::SeqCst);
            p2.more.notify_all();
        });
        // the reaper: notices the end of the program even when something else still holds the terminal open
        let p3 = p.clone();
        std::thread::spawn(move || loop {
            let status = p3.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait();
            match status {
                Ok(Some(st)) => {
                    use std::os::unix::process::ExitStatusExt;
                    p3.code.store(st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0)), Ordering::SeqCst);
                    // what the program left running in its group and its session goes with it
                    unsafe { libc::kill(-p3.pid, libc::SIGKILL) };
                    kill_session(p3.pid);
                    // give the reader a moment to collect the last output, then end for good
                    std::thread::sleep(Duration::from_millis(300));
                    p3.alive.store(false, Ordering::SeqCst);
                    p3.more.notify_all();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(150)),
                Err(_) => return,
            }
        });
        Ok(p)
    }

    fn touch(&self) {
        self.active.store(self.started.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// The exit code once the program has ended (128 + signal when a signal ended it).
    pub fn exit_code(&self) -> Option<i32> {
        let c = self.code.load(Ordering::SeqCst);
        (c >= 0 && !self.alive()).then_some(c)
    }

    pub fn age(&self) -> Duration {
        self.started.elapsed()
    }

    /// Time since the last input or output.
    pub fn idle(&self) -> Duration {
        self.started.elapsed().saturating_sub(Duration::from_millis(self.active.load(Ordering::Relaxed)))
    }

    pub fn size(&self) -> (u16, u16) {
        *self.size.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Types bytes into the terminal. Input beyond `MAX_INPUT` is cut.
    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        if !self.alive() {
            return Err("The terminal has ended.".into());
        }
        let data = &data[..data.len().min(MAX_INPUT)];
        let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        w.write_all(data).and_then(|_| w.flush()).map_err(|e| format!("Could not type into the terminal: {e}"))?;
        drop(w);
        self.touch();
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.clamp(2, 500), cols.clamp(10, 2000));
        let ws = winsize(rows, cols);
        let fd = self.writer.lock().unwrap_or_else(|e| e.into_inner()).as_raw_fd();
        // SAFETY: fd is the open master; ws is a valid winsize.
        unsafe { libc::ioctl(fd, libc::TIOCSWINSZ as _, &ws) };
        *self.size.lock().unwrap_or_else(|e| e.into_inner()) = (rows, cols);
    }

    /// The output from sequence number `from` on, waiting up to `wait` when there is none yet. Asking for more than exists gives
    /// nothing; asking for what was dropped gives what is left (see `Chunk::start`).
    pub fn read_from(&self, from: u64, wait: Duration) -> Chunk {
        let deadline = Instant::now() + wait;
        let mut g = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let end = g.base + g.buf.len() as u64;
            if from < end {
                let start = from.max(g.base);
                let skip = (start - g.base) as usize;
                let data: Vec<u8> = g.buf.iter().skip(skip).take(64 * 1024).copied().collect();
                let next = start + data.len() as u64;
                return Chunk { start, data, next, ended: false };
            }
            if !self.alive() {
                return Chunk { start: end, data: Vec::new(), next: end, ended: true };
            }
            let now = Instant::now();
            if now >= deadline {
                return Chunk { start: end, data: Vec::new(), next: end, ended: false };
            }
            g = self.more.wait_timeout(g, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Sequence number after the last byte so far.
    pub fn end_seq(&self) -> u64 {
        let g = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        g.base + g.buf.len() as u64
    }

    /// All kept output as text (lossy), newest last: for a short login screen.
    pub fn snapshot_text(&self) -> String {
        let g = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        let v: Vec<u8> = g.buf.iter().copied().collect();
        String::from_utf8_lossy(&v).into_owned()
    }

    /// Ends the program and everything it started (its whole process group) and waits for it.
    pub fn close(&self) {
        // SAFETY: a signal to the group this program leads (only while it has not been reaped: its id could be reused after that).
        if self.code.load(Ordering::SeqCst) < 0 {
            unsafe { libc::kill(-self.pid, libc::SIGKILL) };
            // the shell's own jobs (other process groups of its session) too, while the unreaped shell still holds the session's number
            kill_session(self.pid);
        }
        let mut c = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = c.kill();
        if let Ok(st) = c.wait() {
            use std::os::unix::process::ExitStatusExt;
            self.code.compare_exchange(-1, st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0)), Ordering::SeqCst, Ordering::SeqCst).ok();
        }
        drop(c);
        self.alive.store(false, Ordering::SeqCst);
        self.more.notify_all();
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // nobody holds the terminal any more: nothing it started may go on running
        if self.code.load(Ordering::SeqCst) < 0 {
            unsafe { libc::kill(-self.pid, libc::SIGKILL) };
            kill_session(self.pid);
        }
        if let Ok(c) = self.child.get_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Removes terminal control sequences (colours, cursor moves, titles) from output so that it can be read as plain text.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '\x1b' {
            i += 1;
            match b.get(i) {
                Some('[') => {
                    i += 1;
                    while i < b.len() && !(b[i] >= '@' && b[i] <= '~') {
                        i += 1;
                    }
                    i += 1;
                }
                Some(']') => {
                    // an OSC runs to BEL or ESC \
                    i += 1;
                    while i < b.len() && b[i] != '\x07' && b[i] != '\x1b' {
                        i += 1;
                    }
                    if b.get(i) == Some(&'\x1b') {
                        i += 1;
                    }
                    i += 1;
                }
                Some(_) => i += 1,
                None => {}
            }
        } else if c == '\r' {
            i += 1;
        } else if c.is_control() && c != '\n' && c != '\t' {
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Spec {
        let mut s = Spec::new("/bin/sh");
        s.args = vec!["-c".into(), script.into()];
        s.clear_env = true;
        s.env = vec![("PATH".into(), "/usr/bin:/bin".into()), ("TERM".into(), "xterm-256color".into())];
        s
    }

    fn drain(p: &Pty, secs: u64) -> String {
        let (mut seq, mut out) = (0, Vec::new());
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            let c = p.read_from(seq, Duration::from_millis(200));
            out.extend(c.data);
            seq = c.next;
            if c.ended {
                break;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn output_is_kept_and_the_end_is_reported_with_the_exit_code() {
        let p = Pty::spawn(&sh("echo hello-$((6*7)); exit 3")).unwrap();
        let t = drain(&p, 8);
        assert!(t.contains("hello-42"), "{t}");
        assert_eq!(p.exit_code(), Some(3));
    }

    #[test]
    fn input_reaches_the_program_and_a_reconnect_gets_what_follows_its_sequence_number() {
        let p = Pty::spawn(&sh("read x; echo got-$x; sleep 5")).unwrap();
        p.write(b"abc\n").unwrap();
        let first = p.read_from(0, Duration::from_secs(5));
        assert!(!first.data.is_empty());
        std::thread::sleep(Duration::from_millis(500));
        let all = String::from_utf8_lossy(&p.read_from(0, Duration::from_secs(1)).data).into_owned();
        assert!(all.contains("got-abc"), "{all}");
        // a client that already has the first `n` bytes asks from there: nothing is repeated
        let n = all.len() as u64;
        assert!(p.read_from(n, Duration::from_millis(50)).data.is_empty());
        p.close();
        assert!(!p.alive());
    }

    #[test]
    fn the_program_gets_only_the_environment_it_was_given() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ZO_TEST_PTY_SECRET", "must-not-leak-12345");
        let mut s = sh("env; echo ---; cat /proc/$$/environ | tr '\\0' '\\n'");
        s.env.push(("ZO_VISIBLE".into(), "yes".into()));
        let p = Pty::spawn(&s).unwrap();
        let t = drain(&p, 8);
        std::env::remove_var("ZO_TEST_PTY_SECRET");
        assert!(t.contains("ZO_VISIBLE=yes"), "{t}");
        assert!(!t.contains("must-not-leak") && !t.contains("ZO_TEST_PTY_SECRET"), "{t}");
    }

    #[test]
    fn a_terminal_does_not_inherit_the_master_of_another() {
        // the first terminal sits there; the second lists its own descriptors: only 0, 1, 2 (and the one ls itself opens)
        let a = Pty::spawn(&sh("sleep 20")).unwrap();
        let b = Pty::spawn(&sh("ls /proc/self/fd | tr '\\n' ' '")).unwrap();
        let t = drain(&b, 8);
        a.close();
        let fds: Vec<&str> = t.split_whitespace().collect();
        assert!(fds.iter().all(|f| ["0", "1", "2", "3"].contains(f)), "unexpected descriptors: {t}");
    }

    #[test]
    fn closing_kills_the_whole_group() {
        let p = Pty::spawn(&sh("sleep 300 & echo child=$!; wait")).unwrap();
        let t = drain(&p, 3);
        let pid: i32 = t.split("child=").nth(1).and_then(|s| s.split_whitespace().next()).and_then(|s| s.parse().ok()).expect(&t);
        assert!(unsafe { libc::kill(pid, 0) } == 0, "the background program runs");
        p.close();
        std::thread::sleep(Duration::from_millis(400));
        let alive = unsafe { libc::kill(pid, 0) } == 0 && std::fs::read_to_string(format!("/proc/{pid}/stat")).map(|s| !s.contains(") Z")).unwrap_or(false);
        assert!(!alive, "the background program was killed with its group");
    }

    #[test]
    fn the_ring_is_bounded_and_says_what_was_dropped() {
        let p = Pty::spawn(&sh("head -c 3000000 /dev/zero | tr '\\0' 'x'; echo END")).unwrap();
        let end = Instant::now() + Duration::from_secs(20);
        while p.alive() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_millis(400));
        let c = p.read_from(0, Duration::from_millis(100));
        assert!(c.start > 0, "the first bytes were dropped");
        assert!(p.end_seq() - c.start <= RING_BYTES as u64);
    }

    #[test]
    fn limits_are_applied() {
        let mut s = sh("ulimit -n; ulimit -c; ulimit -f");
        s.limits = Some(Limits { nofile: Some(77), fsize: Some(4096 * 512), ..Default::default() });
        let p = Pty::spawn(&s).unwrap();
        let t = drain(&p, 8);
        let lines: Vec<&str> = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, ["77", "0", "4096"], "{t}");
    }

    #[test]
    fn signals_the_parent_ignores_work_again_in_the_terminal() {
        // SAFETY: this test process ignores SIGINT for a moment (as a background job of a shell would) and restores it.
        let old = unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
        let p = Pty::spawn(&sh("grep -E 'SigIgn|SigBlk' /proc/self/status"));
        unsafe { libc::signal(libc::SIGINT, old) };
        let t = drain(&p.unwrap(), 8);
        assert!(t.contains("SigIgn:\t0000000000000000") && t.contains("SigBlk:\t0000000000000000"), "{t}");
    }

    #[test]
    fn control_sequences_are_stripped() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m \x1b]0;title\x07ok\r\n"), "red ok\n");
    }
}
