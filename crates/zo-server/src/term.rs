//! Terminals in the web page (xterm.js) and, through the same four calls, on the person's own computer.
//!
//! * **Cloud** (`target: "cloud"`): a login shell on a pseudo-terminal in the server's container. ADMIN ONLY: the operator's own
//!   addresses (`ZO_ADMIN_EMAILS`), because a shell on a shared server can read what the server's user can read. Everybody else never
//!   gets one, whatever the page sends. `ZO_TERMINAL=off` turns the whole thing off. The shell gets an EMPTY environment plus a
//!   handful of harmless variables (so the server's secrets, even if some were still in its environment, are not there), a home of
//!   its own outside the people's folders, resource limits, a process group of its own that dies with the session, an idle timeout
//!   (30 minutes), a lifetime (8 hours), at most 4 per person and a few for the whole server. The server itself is marked not
//!   dumpable (`hardening::protect_memory`) so that the shell cannot read its memory or environment through /proc.
//! * **My computer** (`target: "desktop"`): the job goes to the person's desktop app through the desktop link; the app asks its owner
//!   ON THAT COMPUTER before it opens anything (`ask_local`, never answerable from the web), then streams the terminal back.
//!
//! The server has no WebSocket: `open` answers with a long NDJSON stream (`out` lines with base64 output and a running sequence number,
//! `ping`, `exit`); input, resizing and closing are small POSTs naming the session. A page that lost its connection opens again with
//! `session` and `from` (the sequence number it has) and gets what it missed, as long as the shell lives. Every call has the same
//! sign-in check as the rest of the API.

use crate::link::Relay;
use crate::tenants::Bucket;
use crate::{App, Fail, Who};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::Request;
use zo_core::pty::{Limits, Pty, Spec};

pub const MAX_PER_TENANT: usize = 4;
pub const MAX_ALL: usize = 8;
pub const IDLE: Duration = Duration::from_secs(30 * 60);
pub const LIFETIME: Duration = Duration::from_secs(8 * 3600);
/// How long an ended shell stays listed (so a page that was away learns the exit code).
const ENDED_KEPT: Duration = Duration::from_secs(60);
/// A silent stream gets a `ping` this often.
pub const PING: Duration = Duration::from_secs(15);
/// The most typed input one call may carry.
pub const MAX_INPUT_CALL: usize = 64 * 1024;
/// Calls per second one session takes (typing, pasting, resizing), with a burst.
const CALLS_PER_MIN: u32 = 3000;

/// The words the page may use.
pub const CALLS: [&str; 5] = ["open", "input", "resize", "close", "list"];

/// Is the cloud terminal switched on? Off only with `ZO_TERMINAL=off`.
pub fn enabled() -> bool {
    !std::env::var("ZO_TERMINAL").map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "off" | "0" | "false" | "no")).unwrap_or(false)
}

fn max_all() -> usize {
    std::env::var("ZO_MAX_TERMINALS").ok().and_then(|v| v.trim().parse().ok()).filter(|n| *n > 0).unwrap_or(MAX_ALL)
}

/// How many processes and threads of this user exist now (the limit below counts all of them, the server's too).
fn user_tasks() -> u64 {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getuid cannot fail.
    let me = unsafe { libc::getuid() };
    let Ok(rd) = std::fs::read_dir("/proc") else { return 0 };
    rd.flatten()
        .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
        .filter(|e| e.metadata().is_ok_and(|m| m.uid() == me))
        .map(|e| std::fs::read_dir(e.path().join("task")).map(|t| t.count() as u64).unwrap_or(1))
        .sum()
}

/// The process limit of a shell: RLIMIT_NPROC counts every process and thread of the user, so it is "what exists now plus room for a
/// shell's own programs" (a fork bomb stops there), not a fixed number that a busy computer already exceeds. `ZO_TERM_NPROC` sets it.
fn nproc_limit() -> u64 {
    std::env::var("ZO_TERM_NPROC").ok().and_then(|v| v.trim().parse().ok()).filter(|n| *n >= 64).unwrap_or_else(|| user_tasks() + 1024)
}

fn mem_limit() -> u64 {
    std::env::var("ZO_TERM_MEM_MB").ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|n| *n >= 64).unwrap_or(2048) * 1024 * 1024
}

/// Where one person's shell lives: outside the people's folders, so that taking an idle person off the disk never removes the
/// directory a shell is standing in.
pub fn home_of(root: &Path, tenant: &str) -> PathBuf {
    let mut p = root.as_os_str().to_os_string();
    p.push("-terminals");
    PathBuf::from(p).join(tenant)
}

fn shell_path() -> PathBuf {
    for p in ["/bin/bash", "/usr/bin/bash", "/bin/sh"] {
        if Path::new(p).exists() {
            return p.into();
        }
    }
    "/bin/sh".into()
}

/// How the shell is started: a login shell, an empty environment with only what is listed here, the person's own home, limits.
pub fn shell_spec(home: &Path, rows: u16, cols: u16) -> Spec {
    let shell = shell_path();
    let is_bash = shell.file_name().is_some_and(|n| n == "bash");
    let mut s = Spec::new(&shell);
    if is_bash {
        s.args = vec!["-l".into()];
    }
    s.clear_env = true;
    s.cwd = Some(home.to_path_buf());
    s.rows = rows;
    s.cols = cols;
    s.env = vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into())),
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("SHELL".into(), shell.to_string_lossy().into_owned()),
        ("USER".into(), "zo".into()),
        ("LOGNAME".into(), "zo".into()),
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
    ];
    s.limits = Some(Limits { cpu_secs: Some(4 * 3600), fsize: Some(1024 * 1024 * 1024), nofile: Some(1024), nproc: Some(nproc_limit()), data: Some(mem_limit()) });
    s
}

struct Sess {
    tenant: String,
    name: String,
    pty: Arc<Pty>,
    created: Instant,
    /// Counts the pages that attached: an older stream ends when a newer one attaches.
    attach: Arc<AtomicU64>,
    rate: Mutex<Bucket>,
    ended_at: Mutex<Option<Instant>>,
}

pub struct Hub {
    inner: Mutex<HashMap<String, Sess>>,
    idle: Duration,
    lifetime: Duration,
}

impl Default for Hub {
    fn default() -> Hub {
        Hub::with_limits(IDLE, LIFETIME)
    }
}

fn new_id() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn tag(tenant: &str) -> &str {
    tenant.get(..8).unwrap_or(tenant)
}

/// What a session looks like to its owner.
#[derive(Clone, Debug)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub rows: u16,
    pub cols: u16,
    pub alive: bool,
    pub exit: Option<i32>,
    pub age_secs: u64,
}

impl Info {
    fn json(&self) -> Value {
        json!({"id": self.id, "name": self.name, "rows": self.rows, "cols": self.cols, "alive": self.alive, "exit": self.exit, "age_secs": self.age_secs})
    }
}

impl Hub {
    pub fn with_limits(idle: Duration, lifetime: Duration) -> Hub {
        Hub { inner: Mutex::new(HashMap::new()), idle, lifetime }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Sess>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts a shell for `tenant` (the caller checked that this person is an admin).
    pub fn open(&self, tenant: &str, root: &Path, rows: u16, cols: u16, name: &str) -> Result<Info, Fail> {
        let mut g = self.lock();
        g.retain(|_, s| s.ended_at.lock().unwrap_or_else(|e| e.into_inner()).is_none_or(|t| t.elapsed() < ENDED_KEPT));
        let alive = |s: &Sess| s.pty.alive();
        if g.values().filter(|s| s.tenant == tenant && alive(s)).count() >= MAX_PER_TENANT {
            return Err(Fail::new(429, "You have too many terminals open. Close one first."));
        }
        if g.values().filter(|s| alive(s)).count() >= max_all() {
            return Err(Fail { status: 503, error: "The server has as many terminals open as it allows. Try again later.".into(), retry: Some(30) });
        }
        let home = home_of(root, tenant);
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&home).map_err(|e| Fail::new(500, format!("Could not prepare the terminal's folder: {e}")))?;
        }
        let pty = Pty::spawn(&shell_spec(&home, rows, cols)).map_err(|e| Fail::new(503, e))?;
        let id = new_id();
        let n: String = zo_core::pty::strip_ansi(name).chars().filter(|c| !c.is_control()).take(40).collect();
        let name = if n.trim().is_empty() { format!("Terminal {}", g.values().filter(|s| s.tenant == tenant).count() + 1) } else { n };
        eprintln!("zo-server: terminal opened for {} ({} open on the server)", tag(tenant), g.len() + 1);
        let s = Sess { tenant: tenant.to_string(), name, pty, created: Instant::now(), attach: Arc::new(AtomicU64::new(0)), rate: Mutex::new(Bucket::new(CALLS_PER_MIN, Instant::now())), ended_at: Mutex::new(None) };
        let info = info_of(&id, &s);
        g.insert(id, s);
        Ok(info)
    }

    pub fn list(&self, tenant: &str) -> Vec<Info> {
        let g = self.lock();
        let mut v: Vec<Info> = g.iter().filter(|(_, s)| s.tenant == tenant).map(|(id, s)| info_of(id, s)).collect();
        v.sort_by_key(|i| std::cmp::Reverse(i.age_secs));
        v
    }

    /// One of this person's sessions (never another person's).
    fn get(&self, tenant: &str, id: &str) -> Option<(Arc<Pty>, Arc<AtomicU64>, String)> {
        let g = self.lock();
        g.get(id).filter(|s| s.tenant == tenant).map(|s| (s.pty.clone(), s.attach.clone(), s.name.clone()))
    }

    pub fn has(&self, tenant: &str, id: &str) -> bool {
        self.get(tenant, id).is_some()
    }

    /// Types into a session. Err: why not.
    pub fn input(&self, tenant: &str, id: &str, data: &[u8]) -> Result<(), Fail> {
        let g = self.lock();
        let s = g.get(id).filter(|s| s.tenant == tenant).ok_or_else(|| Fail::new(404, "There is no such terminal."))?;
        s.rate.lock().unwrap_or_else(|e| e.into_inner()).take(Instant::now()).map_err(|w| Fail { status: 429, error: "Too many calls to this terminal. Slow down.".into(), retry: Some(w) })?;
        let pty = s.pty.clone();
        drop(g);
        pty.write(data).map_err(|e| Fail::new(409, e))
    }

    pub fn resize(&self, tenant: &str, id: &str, rows: u16, cols: u16) -> Result<(), Fail> {
        let (pty, _, _) = self.get(tenant, id).ok_or_else(|| Fail::new(404, "There is no such terminal."))?;
        pty.resize(rows, cols);
        Ok(())
    }

    pub fn close(&self, tenant: &str, id: &str) -> bool {
        let s = {
            let mut g = self.lock();
            if !g.get(id).is_some_and(|s| s.tenant == tenant) {
                return false;
            }
            g.remove(id)
        };
        if let Some(s) = s {
            s.pty.close();
            eprintln!("zo-server: terminal closed for {}", tag(tenant));
        }
        true
    }

    /// Every terminal of a person ends (the account was deleted, or the person is no longer an admin).
    pub fn close_all(&self, tenant: &str) {
        let ids: Vec<String> = self.lock().iter().filter(|(_, s)| s.tenant == tenant).map(|(i, _)| i.clone()).collect();
        for id in ids {
            self.close(tenant, &id);
        }
    }

    /// Housekeeping, every half minute: idle and old shells end, ended ones are forgotten after a while.
    pub fn sweep(&self) {
        let mut doomed = Vec::new();
        {
            let g = self.lock();
            for (id, s) in g.iter() {
                if !s.pty.alive() {
                    s.ended_at.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Instant::now);
                } else if s.pty.idle() > self.idle {
                    doomed.push((id.clone(), s.tenant.clone(), "idle"));
                } else if s.created.elapsed() > self.lifetime {
                    doomed.push((id.clone(), s.tenant.clone(), "too old"));
                }
            }
        }
        for (id, tenant, why) in doomed {
            eprintln!("zo-server: terminal of {} ended ({why})", tag(&tenant));
            self.close(&tenant, &id);
        }
        self.lock().retain(|_, s| s.ended_at.lock().unwrap_or_else(|e| e.into_inner()).is_none_or(|t| t.elapsed() < ENDED_KEPT));
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.lock().len()
    }

    #[cfg(test)]
    pub fn pty_of(&self, tenant: &str, id: &str) -> Option<Arc<Pty>> {
        self.get(tenant, id).map(|x| x.0)
    }
}

fn info_of(id: &str, s: &Sess) -> Info {
    let (rows, cols) = s.pty.size();
    Info { id: id.to_string(), name: s.name.clone(), rows, cols, alive: s.pty.alive(), exit: s.pty.exit_code(), age_secs: s.created.elapsed().as_secs() }
}

// ---------------------------------------------------------------------------------------------------------------------------
// HTTP

fn dim(v: &Value, k: &str, default: u16, lo: u16, hi: u16) -> u16 {
    v.get(k).and_then(Value::as_u64).map(|n| n.clamp(lo as u64, hi as u64) as u16).unwrap_or(default)
}

fn ok(app: &App, req: Request, body: Value) {
    crate::reply(app, req, 200, body);
}

/// Who may open a terminal in the cloud: an admin, and only while the switch is on. Everybody else is told why.
fn cloud_allowed(who: &Who) -> Result<(), Fail> {
    if !enabled() {
        return Err(Fail::new(403, "The terminal is switched off on this server."));
    }
    if !who.admin {
        return Err(Fail::new(403, "A terminal on the cloud server is only for the owner's account. To get a terminal on your own computer, choose My computer under the chat box."));
    }
    Ok(())
}

/// The state of the terminal feature for the page: what the person may do.
pub fn list_http(app: &App, who: &Who) -> Value {
    let cloud = enabled() && who.admin;
    json!({"ok": true, "cloud": cloud, "cloud_enabled": enabled(), "sessions": if cloud { app.term.list(&who.tenant).iter().map(Info::json).collect::<Vec<_>>() } else { Vec::new() }})
}

fn session_of(v: &Value) -> Option<&str> {
    v.get("session").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
}

/// `input`, `resize`, `close` and `list`: small calls with a small answer. `open` streams, see `open_http`.
pub fn call_http(app: &App, req: Request, name: &str, v: &Value, who: &Who) {
    let fail = |req: Request, f: Fail| crate::fail(app, req, f);
    if name == "list" {
        return ok(app, req, list_http(app, who));
    }
    let Some(id) = session_of(v) else { return fail(req, Fail::new(400, "Name the terminal (session).")) };
    let cloud = app.term.has(&who.tenant, id);
    if cloud {
        // somebody who stopped being an admin (or a switch turned off) keeps no shell
        if let Err(f) = cloud_allowed(who) {
            app.term.close_all(&who.tenant);
            return fail(req, f);
        }
    }
    match name {
        "input" => {
            let data = match v.get("data").and_then(Value::as_str) {
                Some(d) if d.len() <= MAX_INPUT_CALL => d.as_bytes().to_vec(),
                Some(_) => return fail(req, Fail::new(413, "That is too much to type at once.")),
                None => return fail(req, Fail::new(400, "Send the text to type as data.")),
            };
            if cloud {
                match app.term.input(&who.tenant, id, &data) {
                    Ok(()) => ok(app, req, json!({"ok": true})),
                    Err(f) => fail(req, f),
                }
            } else {
                match app.link.term_control(&who.tenant, id, json!({"t": "in", "data": String::from_utf8_lossy(&data)})) {
                    Ok(()) => ok(app, req, json!({"ok": true})),
                    Err("too_much_input") => fail(req, Fail { status: 429, error: "Your computer is not keeping up with what you type.".into(), retry: Some(1) }),
                    Err(_) => fail(req, Fail::new(404, "There is no such terminal.")),
                }
            }
        }
        "resize" => {
            let (rows, cols) = (dim(v, "rows", 30, 2, 500), dim(v, "cols", 110, 10, 2000));
            let r = if cloud {
                app.term.resize(&who.tenant, id, rows, cols)
            } else {
                app.link.term_control(&who.tenant, id, json!({"t": "resize", "rows": rows, "cols": cols})).map_err(|_| Fail::new(404, "There is no such terminal."))
            };
            match r {
                Ok(()) => ok(app, req, json!({"ok": true})),
                Err(f) => fail(req, f),
            }
        }
        "close" => {
            let closed = if cloud {
                app.term.close(&who.tenant, id)
            } else {
                app.link.term_control(&who.tenant, id, json!({"t": "close"})).is_ok()
            };
            if closed {
                ok(app, req, json!({"ok": true}))
            } else {
                fail(req, Fail::new(404, "There is no such terminal."))
            }
        }
        _ => fail(req, Fail::new(404, "not found")),
    }
}

fn head() -> String {
    // no cross-origin permission: only this page's own script talks to a terminal
    "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Accel-Buffering: no\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_string()
}

type Out = Box<dyn Write + Send>;

fn chunk(out: &mut Out, v: &Value) -> bool {
    let line = format!("{v}\n");
    out.write_all(format!("{:x}\r\n{line}\r\n", line.len()).as_bytes()).is_ok() && out.flush().is_ok()
}

fn ndjson(app: &App, req: Request, lines: &[Value]) {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let r = tiny_http::Response::from_string(body).with_header(crate::header("Content-Type", "application/x-ndjson")).with_header(crate::header("Cache-Control", "no-store"));
    crate::send_response(app, req, r);
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// POST /api/term/open: starts (or re-attaches to) a terminal and streams it until it ends or the page goes away. A page that goes
/// away leaves a cloud shell running (it can come back); a terminal on a computer is closed.
pub fn open_http(app: &Arc<App>, req: Request, v: Value, who: &Who) {
    let (rows, cols) = (dim(&v, "rows", 30, 2, 500), dim(&v, "cols", 110, 10, 2000));
    let desktop = v.get("target").and_then(Value::as_str) == Some("desktop");
    if desktop {
        return open_desktop(app, req, v, who, rows, cols);
    }
    if let Err(f) = cloud_allowed(who) {
        return crate::fail(app, req, f);
    }
    let info = match session_of(&v) {
        // a page that lost its connection asks for its shell again
        Some(id) => match app.term.get(&who.tenant, id) {
            Some((pty, _, name)) => {
                let (rows, cols) = pty.size();
                Info { id: id.to_string(), name, rows, cols, alive: pty.alive(), exit: pty.exit_code(), age_secs: 0 }
            }
            None => return ndjson(app, req, &[json!({"event": "error", "error": "gone", "message": "That terminal has ended (the server closes idle ones after 30 minutes, and a restart ends all of them)."}), json!({"event": "done"})]),
        },
        None => match app.term.open(&who.tenant, &app.term_root, rows, cols, v.get("name").and_then(Value::as_str).unwrap_or("")) {
            Ok(i) => i,
            Err(f) => return ndjson(app, req, &[json!({"event": "error", "error": "limit", "message": f.error}), json!({"event": "done"})]),
        },
    };
    let Some((pty, attach, _)) = app.term.get(&who.tenant, &info.id) else { return crate::fail(app, req, Fail::new(500, "The terminal vanished.")) };
    let mine = attach.fetch_add(1, Ordering::SeqCst) + 1;
    let mut next = v.get("from").and_then(Value::as_u64).unwrap_or(0);
    let mut out = req.into_writer();
    let mut ok = out.write_all(head().as_bytes()).is_ok() && chunk(&mut out, &json!({"event": "term", "id": info.id, "target": "cloud", "name": info.name, "rows": info.rows, "cols": info.cols}));
    while ok {
        if attach.load(Ordering::SeqCst) != mine {
            let _ = chunk(&mut out, &json!({"event": "replaced"}));
            break;
        }
        let c = pty.read_from(next, PING);
        if !c.data.is_empty() {
            if c.start > next {
                ok = chunk(&mut out, &json!({"event": "lost", "bytes": c.start - next}));
            }
            ok = ok && chunk(&mut out, &json!({"event": "out", "seq": c.start, "b": b64(&c.data)}));
            next = c.next;
        } else if c.ended {
            let _ = chunk(&mut out, &json!({"event": "exit", "code": pty.exit_code().unwrap_or(-1)}));
            let _ = chunk(&mut out, &json!({"event": "done"}));
            break;
        } else {
            ok = chunk(&mut out, &json!({"event": "ping"}));
        }
    }
    let _ = out.write_all(b"0\r\n\r\n");
    let _ = out.flush();
}

fn open_desktop(app: &Arc<App>, req: Request, v: Value, who: &Who, rows: u16, cols: u16) {
    let want = v.get("device").and_then(Value::as_str).filter(|d| !d.is_empty() && d.len() <= 64);
    let stream = app.next.fetch_add(1, Ordering::SeqCst);
    // the computer decides: it asks its owner on its own screen. Nothing here can answer for them.
    let payload = json!({"rows": rows, "cols": cols});
    let (job, name) = match app.link.submit_term(&who.tenant, want, payload, stream) {
        Ok(x) => x,
        Err(e) => {
            let (code, msg) = match e {
                crate::link::SendErr::Offline => ("desktop_offline", crate::link::OFFLINE_MESSAGE.to_string()),
                crate::link::SendErr::Busy => ("desktop_busy", "Your computer is busy.".to_string()),
                crate::link::SendErr::Limit(m) => ("limit", m.to_string()),
            };
            return ndjson(app, req, &[json!({"event": "error", "error": code, "message": msg}), json!({"event": "done"})]);
        }
    };
    let mut out = req.into_writer();
    let mut ok = out.write_all(head().as_bytes()).is_ok() && chunk(&mut out, &json!({"event": "term", "id": job, "target": "desktop", "device": name, "name": name, "rows": rows, "cols": cols}));
    while ok {
        match app.link.relay_wait(&job, app.link.cfg.ping_every) {
            Relay::Idle => ok = chunk(&mut out, &json!({"event": "ping"})),
            Relay::Events(evs) => {
                for e in evs {
                    ok = ok && chunk(&mut out, &e);
                }
            }
            Relay::End(evs, why) => {
                for e in evs {
                    ok = ok && chunk(&mut out, &e);
                }
                match why {
                    Some(("stopped", _)) | None => {}
                    Some((code, msg)) => ok = ok && chunk(&mut out, &json!({"event": "error", "error": code, "message": msg})),
                }
                let _ = ok && chunk(&mut out, &json!({"event": "done"}));
                break;
            }
        }
    }
    let _ = out.write_all(b"0\r\n\r\n");
    let _ = out.flush();
    // a page that went away closes the terminal on the computer
    app.link.cancel_job(&job);
    app.link.finish(&job, stream);
}
