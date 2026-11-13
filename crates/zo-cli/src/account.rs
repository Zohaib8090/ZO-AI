//! `zo login`, `zo logout`, `zo whoami`, and the choice of where `zo` talks to (local app, or the server with the saved sign-in).
//!
//! The sign-in is zo-core's `login` module: the browser flow (a code shown here, approved on the web page) or e-mail and
//! password (Firebase REST, then a credential for this device). The person never sees or types a device credential.

use crate::net::Target;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use zo_core::login as core;

pub const NOT_SIGNED_IN: &str = "Not signed in or the sign-in was revoked: run zo login";

/// Why a sign-in did not happen.
#[derive(Debug, PartialEq)]
pub enum Fail {
    Cancelled,
    Msg(String),
}

impl From<String> for Fail {
    fn from(s: String) -> Fail {
        Fail::Msg(s)
    }
}

// ---- the device's name ----------------------------------------------------------------------------------------------------

pub fn hostname() -> String {
    let mut buf = [0u8; 256];
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len() - 1) } == 0;
    if !ok {
        return String::new();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

/// "Terminal on <host name>": how this device shows up in the web app's "signed-in devices".
pub fn device_name_for(host: &str) -> String {
    let n = if host.trim().is_empty() { "Terminal".to_string() } else { format!("Terminal on {}", host.trim()) };
    core::clean_device_name(&n).unwrap_or_else(|_| "Terminal".into())
}

pub fn device_name() -> String {
    device_name_for(&hostname())
}

// ---- reading a line (and a password) --------------------------------------------------------------------------------------

/// Switches the echo of a terminal off, and back on when dropped. Does nothing when `fd` is not a terminal.
struct EchoOff {
    fd: i32,
    saved: Option<libc::termios>,
}

impl EchoOff {
    fn new(fd: i32) -> EchoOff {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::isatty(fd) == 1 && libc::tcgetattr(fd, &mut t) == 0 {
                let saved = t;
                t.c_lflag &= !libc::ECHO;
                if libc::tcsetattr(fd, libc::TCSAFLUSH, &t) == 0 {
                    return EchoOff { fd, saved: Some(saved) };
                }
            }
        }
        EchoOff { fd, saved: None }
    }
    fn active(&self) -> bool {
        self.saved.is_some()
    }
}

impl Drop for EchoOff {
    fn drop(&mut self) {
        if let Some(t) = &self.saved {
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSAFLUSH, t);
            }
        }
    }
}

/// Reads one line from `fd`. With `hide` the terminal does not echo what is typed. Looks at `cancelled` every 100 ms (a
/// Ctrl-C stops the wait; the echo is restored). End of input before any character is an error.
pub fn read_line_fd(fd: i32, hide: bool, cancelled: &dyn Fn() -> bool) -> Result<String, Fail> {
    let guard = if hide { Some(EchoOff::new(fd)) } else { None };
    let mut bytes: Vec<u8> = Vec::new();
    let res = loop {
        if cancelled() {
            break Err(Fail::Cancelled);
        }
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut p, 1, 100) };
        if r < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break Err(Fail::Msg("Cannot read from the terminal.".into()));
        }
        if r == 0 {
            continue;
        }
        let mut b = [0u8; 1];
        let n = unsafe { libc::read(fd, b.as_mut_ptr() as *mut libc::c_void, 1) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break Err(Fail::Msg("Cannot read from the terminal.".into()));
        }
        if n == 0 {
            if bytes.is_empty() {
                break Err(Fail::Msg("No input.".into()));
            }
            break Ok(());
        }
        if b[0] == b'\n' {
            break Ok(());
        }
        if bytes.len() >= 1024 {
            break Err(Fail::Msg("That line is too long.".into()));
        }
        bytes.push(b[0]);
    };
    let hidden = guard.as_ref().map(EchoOff::active).unwrap_or(false);
    drop(guard);
    if hidden {
        // the Enter key was not echoed
        eprintln!();
    }
    res?;
    while bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn prompt(label: &str, hide: bool, cancelled: &dyn Fn() -> bool) -> Result<String, Fail> {
    eprint!("{label}");
    let _ = std::io::stderr().flush();
    read_line_fd(0, hide, cancelled)
}

// ---- signing in -----------------------------------------------------------------------------------------------------------

/// Waits `secs` seconds in 100 ms steps; stops early when `cancel` is set.
pub fn wait_cancellable(secs: u64, cancel: &AtomicBool) {
    for _ in 0..secs * 10 {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The two lines the person needs, as text.
pub fn code_message(user_code: &str, verify_url: &str) -> String {
    format!(
        "To sign in, open this page in a browser and type the code below there:\n  {}\n\n  Code:  {}\n\nOnly approve it if you started this yourself, in this terminal.\nWaiting for you to approve it (Ctrl-C cancels)...",
        crate::proto::clean(verify_url),
        crate::proto::clean(user_code)
    )
}

/// Tries to open the page in a browser. Failure is fine: the address is printed anyway.
pub fn open_in_browser(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost")) || url.starts_with('-') {
        return;
    }
    let _ = std::process::Command::new("xdg-open").arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map(|mut c| {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    });
}

/// The browser sign-in. `ints` counts Ctrl-C presses; one press cancels. `sleep` waits between polls.
pub fn browser_login(
    base: &str,
    name: &str,
    ints: &(dyn Fn() -> usize + Sync),
    open: Arc<dyn Fn(&str) + Send + Sync>,
    sleep: fn(u64, &AtomicBool),
) -> Result<core::Login, Fail> {
    let start = ints();
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (base, name, cancel2) = (base.to_string(), name.to_string(), cancel.clone());
    // a thread nobody joins: if the person cancels in the middle of a request, the process just ends
    std::thread::spawn(move || {
        let show = move |code: &str, url: &str| {
            eprintln!("{}", code_message(code, url));
            open(url);
        };
        let r = core::run_device_flow_at(&base, &name, "cli", show, &cancel2, &|s, c| sleep(s, c));
        let _ = tx.send(r);
    });
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(l)) => return Ok(l),
            Ok(Err(e)) if e == "Cancelled." => return Err(Fail::Cancelled),
            Ok(Err(e)) => return Err(Fail::Msg(e)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if ints() > start {
                    cancel.store(true, Ordering::SeqCst);
                    return Err(Fail::Cancelled);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Fail::Msg("The sign-in stopped unexpectedly.".into())),
        }
    }
}

/// Sign in with e-mail and password: asks for both (the password is not shown), signs in at Firebase, then trades that for
/// this device's credential. `fb` and `server` are the addresses of Firebase REST and of the Zohara Chat server.
pub fn email_login(fb: &str, server: &str, name: &str, email: &str, password: &str) -> Result<core::Login, Fail> {
    let s = core::sign_in_password_at(fb, email, password)?;
    if s.email_verified == Some(false) {
        let sent = core::send_verification_at(fb, &s.id_token).is_ok();
        return Err(Fail::Msg(format!(
            "The e-mail address {} is not verified yet.{} Click the link in that message, then run zo login --email again.",
            crate::proto::clean(&s.email),
            if sent { " A verification message was sent to it." } else { "" }
        )));
    }
    // (a message about the terms already says where to accept them)
    core::exchange_at(server, &s.id_token, name, "cli").map_err(Fail::Msg)
}

fn finish(l: Result<core::Login, Fail>) -> i32 {
    match l {
        Ok(l) => match core::save_login_as(core::Client::Cli, &l) {
            Ok(()) => {
                let who = if l.email.is_empty() { "your account".to_string() } else { crate::proto::clean(&l.email) };
                println!("Signed in as {who}. Chats, memory and personalities now match the web version.");
                0
            }
            Err(e) => {
                eprintln!("zo: {e}");
                1
            }
        },
        Err(Fail::Cancelled) => {
            eprintln!("zo: cancelled");
            130
        }
        Err(Fail::Msg(e)) => {
            eprintln!("zo: {}", crate::proto::clean(&e));
            1
        }
    }
}

/// `zo login [--email]`. Returns the exit code.
pub fn cmd_login(email: bool, ints: &(dyn Fn() -> usize + Sync)) -> i32 {
    if let Some(who) = core::signed_in_as(core::Client::Cli) {
        println!("Already signed in as {}. Run `zo logout` first to use another account.", crate::proto::clean(&who));
        return 0;
    }
    let server = match core::server_url() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zo: {e}");
            return 1;
        }
    };
    let name = device_name();
    if !email {
        let open: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(open_in_browser);
        return finish(browser_login(&server, &name, ints, open, wait_cancellable));
    }
    let start = ints();
    let cancelled = move || ints() > start;
    let e = match prompt("E-mail: ", false, &cancelled) {
        Ok(e) => e,
        Err(f) => return finish(Err(f)),
    };
    let p = match prompt("Password: ", true, &cancelled) {
        Ok(p) => p,
        Err(f) => return finish(Err(f)),
    };
    finish(email_login(core::FIREBASE_BASE, &server, &name, &e, &p))
}

/// `zo logout`.
pub fn cmd_logout() -> i32 {
    if core::signed_in_as(core::Client::Cli).is_none() && core::credential_of(core::Client::Cli).is_none() {
        println!("Not signed in.");
        return 0;
    }
    match core::sign_out_as(core::Client::Cli) {
        Ok(()) => {
            println!("Signed out. Chats, memory and personalities on this computer stay as they are.");
            0
        }
        Err(e) => {
            eprintln!("zo: {}", crate::proto::clean(&e));
            1
        }
    }
}

/// `zo whoami`: exit 0 and the account when signed in, exit 1 when not.
pub fn cmd_whoami() -> i32 {
    match core::signed_in_as(core::Client::Cli) {
        Some(who) => {
            let server = core::server_url().unwrap_or_else(|e| format!("({e})"));
            println!("{}\nserver  {}", crate::proto::clean(&who), crate::proto::clean(&server));
            0
        }
        None => {
            eprintln!("Not signed in. Run `zo login`.");
            1
        }
    }
}

// ---- where zo talks to ----------------------------------------------------------------------------------------------------

/// What the sign-in saved on this computer: the server and the device credential.
pub type Saved = Option<(String, String)>;

pub fn saved_login() -> Result<Saved, String> {
    if core::signed_in_as(core::Client::Cli).is_none() {
        return Ok(None);
    }
    let Some(cred) = core::credential_of(core::Client::Cli) else { return Ok(None) };
    Ok(Some((core::server_url()?, cred)))
}

/// Checks a device credential given by file or environment: it has to be one the server issued (`zd_...`).
pub fn check_credential(t: &str) -> Result<String, String> {
    let t = t.trim();
    if !t.starts_with("zd_") {
        return Err("That is not a device credential: it must start with zd_. Run `zo login` to sign in instead of copying a token.".into());
    }
    zo_core::sync::check_device_token(t).map_err(|_| "That device credential looks incomplete or damaged. Run `zo login` to sign in again.".to_string())
}

/// Where `zo` talks to, in this order:
/// 1. `--local`: the local app.
/// 2. an explicit server (`--remote`, `ZO_REMOTE_URL`): with the token from `--token-file`/the environment, else the saved
///    sign-in when it is for that same server, else none (a test server without login needs none).
/// 3. the local app, when its socket answers.
/// 4. otherwise the server with the credential from the environment/file, else the saved sign-in, else the local app (which
///    then says it is not running).
pub struct Inputs<'a> {
    pub local: bool,
    pub remote: Option<String>,
    pub token: &'a dyn Fn() -> Result<Option<String>, String>,
    pub socket_alive: &'a dyn Fn() -> bool,
    pub saved: &'a dyn Fn() -> Result<Saved, String>,
    pub server_url: &'a dyn Fn() -> Result<String, String>,
    pub socket: std::path::PathBuf,
}

pub fn resolve_target(i: &Inputs) -> Result<Target, (String, i32)> {
    if i.local {
        return Ok(Target::Local(i.socket.clone()));
    }
    let fatal = |e: String| (e, 1);
    if let Some(url) = &i.remote {
        let url = zo_core::sync::clean_url(url).map_err(|_| (format!("the server address must start with https:// (http only for localhost); got {}", crate::proto::one_line(url, 80)), 2))?;
        let token = match (i.token)().map_err(fatal)? {
            Some(t) => Some(t),
            None => match (i.saved)().map_err(fatal)? {
                Some((srv, cred)) if zo_core::sync::clean_url(&srv).ok().as_deref() == Some(url.as_str()) => Some(cred),
                _ => None,
            },
        };
        return Ok(Target::Remote { url, token });
    }
    if (i.socket_alive)() {
        return Ok(Target::Local(i.socket.clone()));
    }
    if let Some(t) = (i.token)().map_err(fatal)? {
        return Ok(Target::Remote { url: (i.server_url)().map_err(fatal)?, token: Some(t) });
    }
    if let Some((url, cred)) = (i.saved)().map_err(fatal)? {
        return Ok(Target::Remote { url, token: Some(cred) });
    }
    Ok(Target::Local(i.socket.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::Mutex;

    /// Tests that set environment variables (XDG_*, ZO_*) run one at a time. (zo-core's own lock is private to its tests; this
    /// binary has nothing else that touches them.)
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn t(tok: Option<&'static str>) -> impl Fn() -> Result<Option<String>, String> {
        move || Ok(tok.map(str::to_string))
    }

    fn pick(local: bool, remote: Option<&str>, tok: Option<&'static str>, alive: bool, saved: Option<(&'static str, &'static str)>) -> Target {
        let token = t(tok);
        let sock = move || alive;
        let sv = move || Ok(saved.map(|(a, b)| (a.to_string(), b.to_string())));
        let url = || Ok("https://default.example".to_string());
        let i = Inputs { local, remote: remote.map(str::to_string), token: &token, socket_alive: &sock, saved: &sv, server_url: &url, socket: "/s".into() };
        resolve_target(&i).unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn remote(t: Target) -> (String, Option<String>) {
        match t {
            Target::Remote { url, token } => (url, token),
            Target::Local(_) => panic!("expected remote"),
        }
    }

    #[test]
    fn target_choice() {
        let saved = Some(("https://zo.example", "zd_saved"));
        // --local wins over everything
        assert!(matches!(pick(true, Some("https://x.example"), Some("zd_t"), false, saved), Target::Local(_)));
        // the local app is the default when it runs
        assert!(matches!(pick(false, None, None, true, saved), Target::Local(_)));
        // no local app and a saved sign-in: the server
        assert_eq!(remote(pick(false, None, None, false, saved)), ("https://zo.example".into(), Some("zd_saved".into())));
        // no sign-in at all: local (which says it is not running)
        assert!(matches!(pick(false, None, None, false, None), Target::Local(_)));
        // a token from the environment beats the saved sign-in, and goes to the default server
        assert_eq!(remote(pick(false, None, Some("zd_env"), false, saved)), ("https://default.example".into(), Some("zd_env".into())));
        // --remote is used even when the local app runs; its token is the explicit one
        assert_eq!(remote(pick(false, Some("https://x.example/"), Some("zd_env"), true, saved)), ("https://x.example".into(), Some("zd_env".into())));
        // --remote to the saved server uses the saved sign-in; to another server it does not (the credential must not leak)
        assert_eq!(remote(pick(false, Some("https://zo.example"), None, true, saved)).1.as_deref(), Some("zd_saved"));
        assert_eq!(remote(pick(false, Some("https://other.example"), None, true, saved)).1, None);
        // a test server without login needs no token
        assert_eq!(remote(pick(false, Some("http://127.0.0.1:9"), None, false, None)).1, None);
    }

    #[test]
    fn target_errors() {
        let tok = || Err("bad token".to_string());
        let alive = || false;
        let sv = || Ok(None);
        let url = || Ok(String::new());
        let mk = |remote: Option<&str>, token: &dyn Fn() -> Result<Option<String>, String>, alive: &dyn Fn() -> bool| {
            let i = Inputs { local: false, remote: remote.map(str::to_string), token, socket_alive: alive, saved: &sv, server_url: &url, socket: "/s".into() };
            resolve_target(&i).err()
        };
        assert_eq!(mk(Some("http://example.com"), &tok, &alive).unwrap().1, 2);
        assert_eq!(mk(Some("https://x.example"), &tok, &alive).unwrap(), ("bad token".into(), 1));
        // with no explicit server the token is only looked at when the local app does not answer (a stale variable must not break local use)
        assert!(mk(None, &tok, &|| true).is_none());
        assert_eq!(mk(None, &tok, &alive).unwrap().1, 1);
    }

    #[test]
    fn credentials_must_be_device_credentials() {
        assert!(check_credential("zd_0123456789abcdefghijklmn").is_ok());
        assert_eq!(check_credential("  zd_0123456789abcdefghijklmn\n").unwrap(), "zd_0123456789abcdefghijklmn");
        let e = check_credential("old-global-token-123456789").unwrap_err();
        assert!(e.contains("zo login") && e.contains("zd_"), "{e}");
        assert!(check_credential("zd_short").unwrap_err().contains("zo login"));
        assert!(check_credential("").is_err());
    }

    #[test]
    fn device_names() {
        assert_eq!(device_name_for("laptop"), "Terminal on laptop");
        assert_eq!(device_name_for("  "), "Terminal");
        assert_eq!(device_name_for("a\nb"), "Terminal on ab");
        assert!(device_name_for(&"h".repeat(200)).chars().count() <= 60);
        assert!(!hostname().contains('\0'));
    }

    #[test]
    fn the_code_message_shows_code_and_address_and_a_warning() {
        let m = code_message("ABCD-EFGH", "https://zo.example/device");
        assert!(m.contains("ABCD-EFGH") && m.contains("https://zo.example/device") && m.contains("Only approve") && m.contains("Ctrl-C"));
        assert!(!code_message("A\x1b[31mB", "https://x").contains('\x1b'));
    }

    // ---- reading lines ----------------------------------------------------------------------------------------------------

    fn pipe() -> (i32, i32) {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        (fds[0], fds[1])
    }

    fn put(fd: i32, s: &str) {
        assert_eq!(unsafe { libc::write(fd, s.as_ptr() as *const libc::c_void, s.len()) }, s.len() as isize);
    }

    #[test]
    fn lines_are_read_from_a_pipe() {
        let (r, w) = pipe();
        put(w, "me@x.co\r\nsecond\nlast");
        assert_eq!(read_line_fd(r, true, &|| false), Ok("me@x.co".into()));
        assert_eq!(read_line_fd(r, false, &|| false), Ok("second".into()));
        unsafe { libc::close(w) };
        // the last line may lack the newline; then end of input
        assert_eq!(read_line_fd(r, false, &|| false), Ok("last".into()));
        assert_eq!(read_line_fd(r, false, &|| false), Err(Fail::Msg("No input.".into())));
        unsafe { libc::close(r) };
    }

    #[test]
    fn waiting_for_a_line_can_be_cancelled() {
        let (r, w) = pipe();
        let n = std::cell::Cell::new(0);
        let cancelled = || {
            n.set(n.get() + 1);
            n.get() > 2
        };
        assert_eq!(read_line_fd(r, false, &cancelled), Err(Fail::Cancelled));
        unsafe {
            libc::close(r);
            libc::close(w);
        }
    }

    #[test]
    fn a_runaway_line_is_refused() {
        let (r, w) = pipe();
        // (the pipe holds 64 KB, plenty)
        put(w, &"x".repeat(2000));
        assert!(matches!(read_line_fd(r, false, &|| false), Err(Fail::Msg(m)) if m.contains("too long")));
        unsafe {
            libc::close(r);
            libc::close(w);
        }
    }

    fn lflag(fd: i32) -> libc::tcflag_t {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::tcgetattr(fd, &mut t) }, 0);
        t.c_lflag
    }

    #[test]
    fn the_password_is_not_echoed_and_the_terminal_is_restored() {
        // a pseudo-terminal: the slave is what the program reads, the master is the keyboard
        let (mut master, mut slave) = (0i32, 0i32);
        let ok = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
        assert_eq!(ok, 0);
        assert!(lflag(slave) & libc::ECHO != 0);
        let reader = std::thread::spawn(move || {
            let r = read_line_fd(slave, true, &|| false);
            (r, lflag(slave), slave)
        });
        // wait until the echo is off, then type the password
        let mut off = false;
        for _ in 0..100 {
            if lflag(slave) & libc::ECHO == 0 {
                off = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(off, "echo was never switched off");
        put(master, "hunter2\n");
        let (r, _during, slave) = reader.join().unwrap();
        assert_eq!(r, Ok("hunter2".into()));
        assert!(lflag(slave) & libc::ECHO != 0, "echo was not restored");
        // nothing the person typed came back on the terminal's output (the master reads what the slave would display)
        let mut p = libc::pollfd { fd: master, events: libc::POLLIN, revents: 0 };
        let mut shown = Vec::new();
        while unsafe { libc::poll(&mut p, 1, 200) } > 0 {
            let mut b = [0u8; 256];
            let n = unsafe { libc::read(master, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
            if n <= 0 {
                break;
            }
            shown.extend_from_slice(&b[..n as usize]);
        }
        assert!(!String::from_utf8_lossy(&shown).contains("hunter2"), "the password was echoed");
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
    }

    #[test]
    fn cancelling_a_password_prompt_restores_the_terminal() {
        let (mut master, mut slave) = (0i32, 0i32);
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) }, 0);
        let n = std::cell::Cell::new(0);
        let r = read_line_fd(slave, true, &|| {
            n.set(n.get() + 1);
            n.get() > 3
        });
        assert_eq!(r, Err(Fail::Cancelled));
        assert!(lflag(slave) & libc::ECHO != 0);
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
    }

    // ---- a mock of Firebase REST and the Zohara Chat server ---------------------------------------------------------------------

    #[derive(Default)]
    struct Mock {
        users: BTreeMap<String, (String, bool)>,
        creds: Vec<String>,
        polls: usize,
        verification_mails: usize,
        start_name: String,
        start_kind: String,
        op_bearers: Vec<String>,
        handed_out: bool,
    }

    fn read_request(s: &mut std::net::TcpStream) -> Option<(String, String, Value)> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i;
            }
            let n = s.read(&mut chunk).ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
        let mut body = buf[head_end + 4..].to_vec();
        while body.len() < len {
            let n = s.read(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        let path = head.lines().next()?.split(' ').nth(1)?.split('?').next()?.to_string();
        let bearer = head.lines().find_map(|l| l.to_ascii_lowercase().starts_with("authorization: bearer ").then(|| l[22..].to_string())).unwrap_or_default();
        Some((path, bearer, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }

    fn handle(m: &mut Mock, path: &str, bearer: &str, body: &Value) -> (u16, String) {
        let email = body.get("email").and_then(Value::as_str).unwrap_or("").to_string();
        let fb_err = |c: &str| (400, json!({"error": {"code": 400, "message": c}}).to_string());
        let (code, v) = match path {
            "/fb/accounts:signInWithPassword" => match m.users.get(&email) {
                Some((pw, _)) if Some(pw.as_str()) == body["password"].as_str() => return (200, json!({"idToken": format!("idtok:{email}"), "email": email}).to_string()),
                _ => return fb_err("INVALID_LOGIN_CREDENTIALS"),
            },
            "/fb/accounts:lookup" => {
                let e = body["idToken"].as_str().unwrap_or("").trim_start_matches("idtok:").to_string();
                return match m.users.get(&e) {
                    Some((_, v)) => (200, json!({"users": [{"email": e, "emailVerified": v}]}).to_string()),
                    None => fb_err("INVALID_ID_TOKEN"),
                };
            }
            "/fb/accounts:sendOobCode" => {
                m.verification_mails += 1;
                return (200, json!({"email": "x"}).to_string());
            }
            "/api/auth/exchange" => {
                m.start_name = body["name"].as_str().unwrap_or("").into();
                m.start_kind = body["kind"].as_str().unwrap_or("").into();
                let e = bearer.trim_start_matches("idtok:").to_string();
                let c = format!("zd_exchange_{:0>40}", m.creds.len());
                m.creds.push(c.clone());
                (200, json!({"ok": true, "credential": c, "email": e}))
            }
            "/api/auth/device_start" => {
                m.start_name = body["name"].as_str().unwrap_or("").into();
                m.start_kind = body["kind"].as_str().unwrap_or("").into();
                (200, json!({"ok": true, "device_code": "D".repeat(43), "user_code": "ABCD-EFGH", "verify_url": "https://zo.example/device", "expires_in": 600, "interval": 1}))
            }
            "/api/auth/device_poll" => {
                m.polls += 1;
                if m.polls < 3 {
                    (200, json!({"ok": true, "status": "pending"}))
                } else if m.handed_out {
                    (200, json!({"ok": true, "status": "expired"}))
                } else {
                    m.handed_out = true;
                    let c = format!("zd_browser_{:0>41}", m.creds.len());
                    m.creds.push(c.clone());
                    (200, json!({"ok": true, "status": "approved", "credential": c, "email": "cy@x.co"}))
                }
            }
            "/api/auth/signout" => match m.creds.iter().position(|c| c == bearer) {
                Some(i) => {
                    m.creds.remove(i);
                    (200, json!({"ok": true}))
                }
                None => (401, json!({"ok": false, "error": "Not signed in."})),
            },
            "/api/op" => {
                m.op_bearers.push(bearer.to_string());
                if !m.creds.iter().any(|c| c == bearer) {
                    (401, json!({"ok": false, "error": "no"}))
                } else {
                    (200, json!({"ok": true, "ready": true, "provider": "p", "model": "m", "mode": "Manual", "version": "9"}))
                }
            }
            _ => (404, json!({"ok": false, "error": "no such call"})),
        };
        (code, v.to_string())
    }

    fn start_mock() -> (String, Arc<Mutex<Mock>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let st = Arc::new(Mutex::new(Mock::default()));
        let st2 = st.clone();
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut c) = conn else { continue };
                let st = st2.clone();
                std::thread::spawn(move || {
                    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Some((path, bearer, body)) = read_request(&mut c) {
                        let (code, b) = handle(&mut st.lock().unwrap(), &path, &bearer, &body);
                        let _ = write!(c, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
                    }
                });
            }
        });
        (addr, st)
    }

    fn fast(_: u64, _: &AtomicBool) {
        std::thread::sleep(Duration::from_millis(20));
    }

    fn sandbox(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("zo-cli-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn browser_flow_shows_the_code_and_returns_the_credential() {
        let (addr, st) = start_mock();
        let opened = Arc::new(Mutex::new(Vec::<String>::new()));
        let o2 = opened.clone();
        let open: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |u| o2.lock().unwrap().push(u.to_string()));
        let l = browser_login(&addr, "Terminal on box", &|| 0, open, fast).unwrap();
        assert!(l.credential.starts_with("zd_browser_") && l.email == "cy@x.co");
        assert_eq!(st.lock().unwrap().start_kind, "cli");
        assert_eq!(st.lock().unwrap().start_name, "Terminal on box");
        assert_eq!(*opened.lock().unwrap(), vec!["https://zo.example/device".to_string()]);
    }

    #[test]
    fn ctrl_c_cancels_the_browser_flow_cleanly() {
        let (addr, _st) = start_mock();
        // the mock keeps answering "pending" long enough: the counter goes up after a few reads
        let n = std::sync::atomic::AtomicUsize::new(0);
        let ints = move || n.fetch_add(1, Ordering::SeqCst) / 3;
        fn slow(_: u64, c: &AtomicBool) {
            wait_cancellable(5, c);
        }
        let t0 = std::time::Instant::now();
        let r = browser_login(&addr, "Terminal", &ints, Arc::new(|_| {}), slow);
        assert_eq!(r.unwrap_err(), Fail::Cancelled);
        assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
    }

    #[test]
    fn a_refused_start_is_an_error_not_a_hang() {
        // nothing listens on this port
        let r = browser_login("http://127.0.0.1:9", "Terminal", &|| 0, Arc::new(|_| {}), fast);
        assert!(matches!(r, Err(Fail::Msg(_))));
    }

    #[test]
    fn email_flow() {
        let (addr, st) = start_mock();
        {
            let mut m = st.lock().unwrap();
            m.users.insert("ok@x.co".into(), ("secret1".into(), true));
            m.users.insert("new@x.co".into(), ("secret1".into(), false));
        }
        let fb = format!("{addr}/fb");
        let l = email_login(&fb, &addr, "Terminal on box", "ok@x.co", "secret1").unwrap();
        assert!(l.credential.starts_with("zd_exchange_") && l.email == "ok@x.co");
        let (kind, name) = {
            let m = st.lock().unwrap();
            (m.start_kind.clone(), m.start_name.clone())
        };
        assert_eq!((kind.as_str(), name.as_str()), ("cli", "Terminal on box"));
        // wrong password
        assert_eq!(email_login(&fb, &addr, "T", "ok@x.co", "nope").unwrap_err(), Fail::Msg("Wrong e-mail or password.".into()));
        // not verified: a message is sent and the person is told, no credential is made
        let before = st.lock().unwrap().creds.len();
        let Fail::Msg(e) = email_login(&fb, &addr, "T", "new@x.co", "secret1").unwrap_err() else { panic!() };
        assert!(e.contains("not verified") && e.contains("verification message was sent"), "{e}");
        assert_eq!(st.lock().unwrap().verification_mails, 1);
        assert_eq!(st.lock().unwrap().creds.len(), before);
    }

    /// The whole thing: sign in through the browser flow, see it with whoami, talk to the server with the saved credential,
    /// sign out (the credential is revoked on the server).
    #[test]
    fn sign_in_whoami_remote_call_and_sign_out() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (addr, st) = start_mock();
        let d = sandbox("e2e");
        std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
        std::env::set_var("XDG_DATA_HOME", d.join("data"));
        std::env::set_var("ZOHARA_CHAT_KEYS_FILE", d.join("keys.json"));
        std::env::remove_var("ZOHARA_CHAT_KEY_SYNC_TOKEN");
        std::env::set_var("ZO_SERVER_URL", &addr);
        assert_eq!(cmd_whoami(), 1);
        assert_eq!(cmd_logout(), 0);
        let l = browser_login(&addr, &device_name(), &|| 0, Arc::new(|_| {}), fast).unwrap();
        assert_eq!(finish(Ok(l)), 0);
        assert_eq!(cmd_whoami(), 0);
        assert_eq!(cmd_login(false, &|| 0), 0, "a second login only says it is signed in");
        assert_eq!(st.lock().unwrap().start_name, device_name());
        // no local app listening: the server with the saved credential becomes the target
        let tok = || Ok(None);
        let dead = || false;
        let url = || core::server_url();
        let i = Inputs { local: false, remote: None, token: &tok, socket_alive: &dead, saved: &saved_login, server_url: &url, socket: d.join("none.sock") };
        let target = resolve_target(&i).unwrap();
        assert!(matches!(&target, Target::Remote { url, token: Some(t) } if *url == addr && t.starts_with("zd_browser_")));
        let r = target.op(&json!({"op": "status"})).unwrap();
        assert_eq!(r["model"], "m");
        assert!(st.lock().unwrap().op_bearers[0].starts_with("zd_browser_"));
        // sign out: revoked on the server, forgotten here, and the target falls back to local
        assert_eq!(cmd_logout(), 0);
        assert!(st.lock().unwrap().creds.is_empty());
        assert_eq!(cmd_whoami(), 1);
        assert!(matches!(resolve_target(&i).unwrap(), Target::Local(_)));
        // a revoked credential gives the plain 401 message
        let dead_target = Target::Remote { url: addr.clone(), token: Some("zd_revoked_0123456789abcdef".into()) };
        assert_eq!(dead_target.op(&json!({"op": "status"})).unwrap_err(), NOT_SIGNED_IN);
        for k in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "ZOHARA_CHAT_KEYS_FILE", "ZO_SERVER_URL"] {
            std::env::remove_var(k);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
