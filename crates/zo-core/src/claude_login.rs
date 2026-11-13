//! One-click Claude Code login. The person sees steps (a link to open, a place for the code it gives back, a status), never a token.
//!
//! * **On a computer** (the desktop app): `claude auth login` runs on a pseudo-terminal; the authorization link it prints becomes a
//!   button, the code the person gets back is typed into the program, and when it ends `claude auth status` says who is signed in.
//!   The program keeps the login itself, in the person's own home. ZO-AI stores nothing.
//! * **On a server** (`ZO_CLAUDE_CODE_USERS`: admins, or everybody): `claude setup-token` runs on a pseudo-terminal in a sandbox (an
//!   empty environment, a private home, resource limits, five minutes at most, one per person and a few for the whole server). Its
//!   screen is NEVER passed on: the page only gets a state and the authorization link (checked against the sign-in hosts of
//!   Anthropic). The long-lived token the program prints at the end is taken from its output, stored encrypted in the person's own
//!   key store (`claude_code::store_token`) and the program is destroyed. The token is not logged, not returned and not in an error.
//!
//! What the program prints is parsed here (`find_auth_url`, `find_token`); the formats were taken from Claude Code 2.1.x. If a later
//! version prints something else the flow fails with a plain message and the person can use the terminal instead.

use crate::pty::{strip_ansi, Limits, Pty, Spec};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A server-side flow may take this long (the person has to open a page, sign in and copy a code).
pub const FLOW_SECS: u64 = 300;
/// Flows running at once on a server (each is a Node process of roughly 300 MB), unless `ZO_MAX_CLAUDE_LOGINS` says otherwise.
pub const DEFAULT_MAX_FLOWS: usize = 2;
/// The sign-in pages the link may point to.
const HOSTS: [&str; 5] = ["claude.com", "claude.ai", "console.anthropic.com", "platform.claude.com", "anthropic.com"];
/// How long a code may be (what the sign-in page gives back is about 60 characters).
pub const MAX_CODE: usize = 400;

// ---------------------------------------------------------------------------------------------------------------------------
// reading what the program prints

fn host_ok(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    HOSTS.iter().any(|a| h == *a || h.ends_with(&format!(".{a}")))
}

/// The authorization link in the program's raw output (the first link to a sign-in host). Only `https`, no user name, no port.
pub fn find_auth_url(raw: &str) -> Option<String> {
    let mut rest = raw;
    while let Some(i) = rest.find("https://") {
        let tail = &rest[i..];
        let end = tail.find(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`')).unwrap_or(tail.len());
        let url = &tail[..end];
        let host = url["https://".len()..].split(['/', '?', '#']).next().unwrap_or("");
        if url.len() <= 2000 && !host.is_empty() && !host.contains(['@', ':']) && host_ok(host) && url["https://".len() + host.len()..].starts_with('/') {
            return Some(url.to_string());
        }
        rest = &tail["https://".len()..];
    }
    None
}

/// The long-lived token in the program's output (control sequences are removed first).
pub fn find_token(raw: &str) -> Option<String> {
    let text = strip_ansi(raw);
    let mut rest = text.as_str();
    while let Some(i) = rest.find("sk-ant-") {
        let tail = &rest[i..];
        let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(tail.len());
        let t = &tail[..end];
        if crate::ai::claude_code::token_shape_ok(t) && t.starts_with("sk-ant-oat") {
            return Some(t.to_string());
        }
        rest = &tail["sk-ant-".len()..];
    }
    None
}

/// `s` with every token-looking run replaced, for text that may be shown.
pub fn redact(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("sk-ant-") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(tail.len());
        out.push_str("[token]");
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// A code as the person pastes it: one line, printable, not long. What is typed into the program is exactly this plus Enter.
pub fn clean_code(code: &str) -> Result<String, String> {
    let c = code.trim();
    if c.is_empty() {
        return Err("Paste the code the sign-in page gave you.".into());
    }
    if c.len() > MAX_CODE || !c.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err("That does not look like the code from the sign-in page.".into());
    }
    Ok(c.to_string())
}

// ---------------------------------------------------------------------------------------------------------------------------
// what the page or window shows

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Nothing is running.
    Idle,
    /// The program is starting.
    Starting,
    /// The link is ready: open it, sign in, copy the code.
    Link,
    /// The code was given to the program.
    Working,
    Done,
    Failed,
    Cancelled,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Starting => "starting",
            State::Link => "link",
            State::Working => "working",
            State::Done => "done",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }
    pub fn over(self) -> bool {
        matches!(self, State::Done | State::Failed | State::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub struct View {
    pub state: State,
    pub url: Option<String>,
    pub message: String,
    /// Seconds left before the flow is given up.
    pub secs_left: u64,
}

impl View {
    pub fn to_json(&self) -> Value {
        json!({"ok": true, "state": self.state.name(), "url": self.url, "message": self.message, "secs_left": self.secs_left})
    }
}

fn message(s: State, own_error: &str) -> String {
    match s {
        State::Idle => "No sign-in is running.".into(),
        State::Starting => "Starting the sign-in...".into(),
        State::Link => "Open the link, sign in to Claude, then copy the code it shows and paste it here.".into(),
        State::Working => "Checking the code...".into(),
        State::Done => "You are signed in to Claude Code.".into(),
        State::Failed => own_error.to_string(),
        State::Cancelled => "Cancelled.".into(),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// one flow (shared by both places)

struct Flow {
    pty: Arc<Pty>,
    started: Instant,
    state: Mutex<(State, Option<String>, String)>,
    /// A code was typed in.
    code_sent: AtomicBool,
    cancel: AtomicBool,
}

impl Flow {
    fn view(&self) -> View {
        let g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        View { state: g.0, url: g.1.clone(), message: message(g.0, &g.2), secs_left: FLOW_SECS.saturating_sub(self.started.elapsed().as_secs()) }
    }

    fn set(&self, s: State, err: &str) {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if g.0.over() {
            return;
        }
        g.0 = s;
        if s == State::Failed {
            g.2 = err.to_string();
        }
    }

    fn set_url(&self, url: String) {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if g.1.is_none() {
            g.1 = Some(url);
            if g.0 == State::Starting {
                g.0 = State::Link;
            }
        }
    }

    fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0
    }

    fn submit(&self, code: &str) -> Result<(), String> {
        let code = clean_code(code)?;
        if self.state() != State::Link {
            return Err("There is no sign-in waiting for a code right now.".into());
        }
        if self.code_sent.swap(true, Ordering::SeqCst) {
            return Err("A code was already given. Wait for the result, or start again.".into());
        }
        self.pty.write(format!("{code}\r").as_bytes())?;
        self.set(State::Working, "");
        Ok(())
    }
}

fn random_hex() -> String {
    let mut b = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut b);
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn new_flow(spec: &Spec) -> Result<Arc<Flow>, String> {
    let pty = Pty::spawn(spec)?;
    Ok(Arc::new(Flow { pty, started: Instant::now(), state: Mutex::new((State::Starting, None, String::new())), code_sent: AtomicBool::new(false), cancel: AtomicBool::new(false) }))
}

/// Reads the program's output as it comes, until the flow is over. `on_text` looks at everything printed so far and may end the
/// flow (it returns the final state). Ends and destroys the program in every case.
fn watch(flow: &Arc<Flow>, mut on_text: impl FnMut(&Flow, &str) -> Option<State>) {
    let mut seq = 0;
    let mut text = String::new();
    loop {
        if flow.cancel.load(Ordering::SeqCst) {
            flow.set(State::Cancelled, "");
            break;
        }
        if flow.started.elapsed() > Duration::from_secs(FLOW_SECS) {
            flow.set(State::Failed, "The sign-in took too long. Start it again.");
            break;
        }
        let c = flow.pty.read_from(seq, Duration::from_millis(250));
        seq = c.next;
        text.push_str(&String::from_utf8_lossy(&c.data));
        // only what is needed is kept: the link and the token are near the end
        if text.len() > 200_000 {
            let cut = text.len() - 100_000;
            let cut = (cut..text.len()).find(|i| text.is_char_boundary(*i)).unwrap_or(text.len());
            text.drain(..cut);
        }
        if let Some(u) = find_auth_url(&text) {
            flow.set_url(u);
        }
        if let Some(s) = on_text(flow, &text) {
            flow.set(s, "");
            break;
        }
        if c.ended {
            // one more look at the very last output
            if let Some(s) = on_text(flow, &text) {
                flow.set(s, "");
            } else {
                flow.set(State::Failed, "The sign-in ended without finishing. Start it again.");
            }
            break;
        }
    }
    flow.pty.close();
}

// ---------------------------------------------------------------------------------------------------------------------------
// on a computer: `claude auth login`

static DESKTOP: Mutex<Option<Arc<Flow>>> = Mutex::new(None);

fn desktop_flow() -> Option<Arc<Flow>> {
    DESKTOP.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What `claude auth status` says: (logged in, account text). Runs the program for a moment; never prints a token.
pub fn status_of(bin: &std::path::Path, home: Option<&std::path::Path>) -> (bool, String) {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(["auth", "status", "--json"]);
    if let Some(h) = home {
        cmd.env("HOME", h);
    }
    cmd.env("DISABLE_AUTOUPDATER", "1");
    crate::exec::scrub_server(&mut cmd);
    let Ok(out) = crate::exec::run(&mut cmd, Duration::from_secs(15)) else { return (false, String::new()) };
    parse_status(&out.stdout)
}

pub fn parse_status(json_text: &str) -> (bool, String) {
    let v: Value = serde_json::from_str(json_text.trim()).unwrap_or(Value::Null);
    let logged = v.get("loggedIn").and_then(Value::as_bool).unwrap_or(false);
    let who = v.get("email").and_then(Value::as_str).unwrap_or("");
    let plan = v.get("planTier").and_then(Value::as_str).or_else(|| v.get("subscriptionType").and_then(Value::as_str)).unwrap_or("");
    let who: String = who.chars().filter(|c| !c.is_control()).take(120).collect();
    let plan: String = plan.chars().filter(|c| !c.is_control()).take(40).collect();
    let text = match (who.is_empty(), plan.is_empty()) {
        (true, _) => String::new(),
        (false, true) => who,
        (false, false) => format!("{who} ({plan})"),
    };
    (logged, text)
}

/// Where the Claude Code program is on this computer.
fn desktop_bin() -> Result<PathBuf, String> {
    if crate::tenant::multi_tenant() {
        return Err("This is for the desktop app.".into());
    }
    crate::ai::claude_code::binary().ok_or_else(|| "Claude Code is not installed. Install it first: npm install -g @anthropic-ai/claude-code".to_string())
}

/// Starts `claude auth login` on this computer (the person's own account and home). One at a time.
pub fn desktop_start() -> Result<View, String> {
    let bin = desktop_bin()?;
    if let Some(f) = desktop_flow().filter(|f| !f.state().over()) {
        return Ok(f.view());
    }
    let mut spec = Spec::new(&bin);
    spec.args = vec!["auth".into(), "login".into(), "--claudeai".into()];
    spec.cols = 1000;
    spec.env = vec![("TERM".into(), "xterm-256color".into()), ("DISABLE_AUTOUPDATER".into(), "1".into())];
    spec.limits = Some(Limits { cpu_secs: Some(300), ..Default::default() });
    let flow = new_flow(&spec)?;
    *DESKTOP.lock().unwrap_or_else(|e| e.into_inner()) = Some(flow.clone());
    let f = flow.clone();
    std::thread::spawn(move || {
        watch(&f, |f, _| {
            // the program ends by itself when the browser finished the sign-in (or after the code was typed): exit code 0 is success
            if !f.pty.alive() {
                return Some(match f.pty.exit_code() {
                    Some(0) => State::Done,
                    _ => State::Failed,
                });
            }
            None
        });
        // a program that ended with a failure reports a plain message (see `view`), the screen is not shown
    });
    Ok(flow.view())
}

pub fn desktop_view() -> View {
    match desktop_flow() {
        Some(f) => f.view(),
        None => View { state: State::Idle, url: None, message: "No sign-in is running.".into(), secs_left: 0 },
    }
}

pub fn desktop_submit(code: &str) -> Result<View, String> {
    let f = desktop_flow().ok_or("No sign-in is running.")?;
    f.submit(code)?;
    Ok(f.view())
}

pub fn desktop_cancel() {
    if let Some(f) = desktop_flow() {
        f.cancel.store(true, Ordering::SeqCst);
        f.set(State::Cancelled, "");
        f.pty.close();
    }
}

/// Signs out of Claude Code on this computer (`claude auth logout`).
pub fn desktop_logout() -> Result<(), String> {
    let bin = desktop_bin()?;
    let mut cmd = std::process::Command::new(bin);
    cmd.args(["auth", "logout"]).env("DISABLE_AUTOUPDATER", "1");
    crate::exec::scrub_server(&mut cmd);
    let out = crate::exec::run(&mut cmd, Duration::from_secs(20))?;
    if out.code == Some(0) {
        Ok(())
    } else {
        Err("Claude Code could not sign out. Try: claude auth logout".into())
    }
}

/// The state of the sign-in on this computer: (installed, logged in, account text).
pub fn desktop_status() -> (bool, bool, String) {
    match desktop_bin() {
        Err(_) => (false, false, String::new()),
        Ok(bin) => {
            let (l, w) = status_of(&bin, None);
            (true, l, w)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// on a server: `claude setup-token`

fn flows() -> &'static Mutex<HashMap<String, Arc<Flow>>> {
    static F: std::sync::OnceLock<Mutex<HashMap<String, Arc<Flow>>>> = std::sync::OnceLock::new();
    F.get_or_init(|| Mutex::new(HashMap::new()))
}

fn max_flows() -> usize {
    std::env::var("ZO_MAX_CLAUDE_LOGINS").ok().and_then(|v| v.trim().parse().ok()).filter(|n| *n > 0).unwrap_or(DEFAULT_MAX_FLOWS)
}

fn flow_of(id: &str) -> Option<Arc<Flow>> {
    flows().lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
}

fn tenant_flow() -> Option<Arc<Flow>> {
    flow_of(&crate::tenant::current()?.id)
}

/// The program that signs in, started for a server: the operator's own `claude`, in the person's folders.
fn server_spec(bin: PathBuf, t: &crate::tenant::Tenant, home: &std::path::Path) -> Spec {
    let mut spec = Spec::new(bin);
    spec.args = vec!["setup-token".into()];
    spec.clear_env = true;
    spec.cols = 1000;
    spec.rows = 50;
    spec.cwd = Some(t.data.clone());
    spec.env = vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into())),
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("TERM".into(), "xterm-256color".into()),
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
        // there is no browser here: the link is printed
        ("BROWSER".into(), "/bin/true".into()),
    ];
    spec.limits = Some(Limits { cpu_secs: Some(240), fsize: Some(10 * 1024 * 1024), nofile: Some(512), data: Some(1536 * 1024 * 1024), ..Default::default() });
    spec
}

/// Starts the guided sign-in for the current tenant (call it inside `tenant::scope`). Only where `claude_code::allowed_here`.
pub fn server_start() -> Result<View, String> {
    if !crate::ai::claude_code::allowed_here() {
        return Err("Claude Code sign-in is not available to this account on this server.".into());
    }
    let t = crate::tenant::current().ok_or("Claude Code sign-in is not available to this account on this server.")?;
    let bin = crate::ai::claude_code::binary().ok_or("Claude Code is not installed on the server.")?;
    let mut map = flows().lock().unwrap_or_else(|e| e.into_inner());
    // flows that ended are forgotten
    map.retain(|_, f| !f.state().over() || f.started.elapsed() < Duration::from_secs(30));
    if let Some(f) = map.get(&t.id).filter(|f| !f.state().over()) {
        return Ok(f.view());
    }
    if map.values().filter(|f| !f.state().over()).count() >= max_flows() {
        return Err("The server is signing in other people right now. Try again in a minute.".into());
    }
    // a private home that goes away afterwards: the program writes its settings there, and nothing of it may stay
    let home = t.config.join(format!("claude-login-{}", &random_hex()));
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&home).map_err(|e| format!("Could not prepare the sign-in: {e}"))?;
    }
    let flow = match new_flow(&server_spec(bin, &t, &home)) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&home);
            return Err(e);
        }
    };
    map.insert(t.id.clone(), flow.clone());
    drop(map);
    let f = flow.clone();
    crate::tenant::spawn(move || {
        watch(&f, |f, text| {
            if let Some(token) = find_token(text) {
                // stored for this person (encrypted); the program is destroyed right after
                return Some(match crate::ai::claude_code::store_token(&token) {
                    Ok(_) => State::Done,
                    Err(_) => {
                        f.state.lock().unwrap_or_else(|e| e.into_inner()).2 = "The server could not keep the sign-in. Try again.".into();
                        State::Failed
                    }
                });
            }
            None
        });
        let _ = std::fs::remove_dir_all(&home);
        if f.state() == State::Failed && f.state.lock().unwrap_or_else(|e| e.into_inner()).2.is_empty() {
            f.state.lock().unwrap_or_else(|e| e.into_inner()).2 = "The sign-in failed. Start it again.".into();
        }
    });
    Ok(flow.view())
}

pub fn server_view() -> View {
    view_of(tenant_flow())
}

/// The state of the person's flow by their id, for a caller that has no tenant context (a poll that only reads).
pub fn server_view_for(id: &str) -> View {
    view_of(flow_of(id))
}

fn view_of(f: Option<Arc<Flow>>) -> View {
    match f {
        Some(f) => f.view(),
        None => View { state: State::Idle, url: None, message: "No sign-in is running.".into(), secs_left: 0 },
    }
}

pub fn server_submit(code: &str) -> Result<View, String> {
    submit_to(tenant_flow(), code)
}

pub fn server_submit_for(id: &str, code: &str) -> Result<View, String> {
    submit_to(flow_of(id), code)
}

fn submit_to(f: Option<Arc<Flow>>, code: &str) -> Result<View, String> {
    let f = f.ok_or("No sign-in is running.")?;
    f.submit(code)?;
    Ok(f.view())
}

pub fn server_cancel() {
    cancel_flow(tenant_flow());
}

pub fn server_cancel_for(id: &str) {
    cancel_flow(flow_of(id));
}

fn cancel_flow(f: Option<Arc<Flow>>) {
    if let Some(f) = f {
        f.cancel.store(true, Ordering::SeqCst);
        f.set(State::Cancelled, "");
        f.pty.close();
    }
}

/// Signs the current tenant out: the token and the program's folder are removed.
pub fn server_logout() -> Result<(), String> {
    server_cancel();
    crate::ai::claude_code::clear_token()?;
    if let Some(t) = crate::tenant::current() {
        let _ = std::fs::remove_dir_all(t.config.join("claude-home"));
    }
    Ok(())
}

/// How many guided sign-ins are running on this server (tests, status).
pub fn running_flows() -> usize {
    flows().lock().unwrap_or_else(|e| e.into_inner()).values().filter(|f| !f.state().over()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN_OUT: &str = "Opening browser to sign in…\r\nIf the browser didn't open, visit: \x1b]8;;https://claude.com/cai/oauth/authorize?code=true&client_id=abc&state=xyz\x07\x1b[94mhttps://claude.com/cai/oauth/authorize?code=true&client_id=abc&state=xyz\x1b[39m\x1b]8;;\x07\r\nPaste code here if prompted > ";

    #[test]
    fn the_authorization_link_is_found_and_only_sign_in_hosts_count() {
        assert_eq!(find_auth_url(LOGIN_OUT).as_deref(), Some("https://claude.com/cai/oauth/authorize?code=true&client_id=abc&state=xyz"));
        // another host, a user name trick, a port, plain http: none is accepted
        for bad in ["see https://evil.example/claude.com/x", "https://claude.com@evil.example/x", "https://claude.com.evil.example/x", "https://claude.com:444/x", "http://claude.com/x", "https://notclaude.com/x", "https://claude.com"] {
            assert_eq!(find_auth_url(bad), None, "{bad}");
        }
        // a first link to another host does not hide a later real one
        assert_eq!(find_auth_url("https://evil.example/a then https://claude.ai/login?x=1 end").as_deref(), Some("https://claude.ai/login?x=1"));
    }

    #[test]
    fn the_token_is_found_through_control_sequences_and_never_shown() {
        let raw = "Your OAuth token (valid for 1 year):\r\n\x1b[1m\x1b[32msk-ant-oat01-AbCdEf0123456789_-AbCdEf0123456789xyz\x1b[39m\r\nStore this token securely.";
        let t = find_token(raw).unwrap();
        assert_eq!(t, "sk-ant-oat01-AbCdEf0123456789_-AbCdEf0123456789xyz");
        assert!(!redact(raw).contains("AbCdEf"), "{}", redact(raw));
        assert_eq!(find_token("an API key sk-ant-api03-AbCdEf0123456789_-AbCdEf0123456789xyz"), None, "only a subscription token");
        assert_eq!(find_token("sk-ant-oat01-short"), None);
    }

    #[test]
    fn a_code_is_one_plain_line() {
        assert_eq!(clean_code("abc\ndef").unwrap_err(), "That does not look like the code from the sign-in page.");
        assert_eq!(clean_code(" abc-DEF_123#state ").unwrap(), "abc-DEF_123#state");
        assert!(clean_code("").is_err());
        assert!(clean_code(&"a".repeat(MAX_CODE + 1)).is_err());
        assert!(clean_code("a\u{1b}[31mb").is_err());
    }

    #[test]
    fn the_account_comes_from_the_status() {
        assert_eq!(parse_status(r#"{"loggedIn":true,"email":"a@b.c","planTier":"max","orgName":"x"}"#), (true, "a@b.c (max)".to_string()));
        assert_eq!(parse_status(r#"{"loggedIn":false,"authMethod":"none"}"#), (false, String::new()));
        assert_eq!(parse_status("not json"), (false, String::new()));
    }

    /// A stand-in for `claude setup-token` that behaves like the real one on screen: link, then a prompt for the code, then the token.
    fn fake_claude(dir: &std::path::Path, token: &str) -> PathBuf {
        let p = dir.join("claude");
        let script = format!(
            "#!/bin/sh\nprintf 'Welcome to Claude Code\\r\\n'\nprintf 'Browser didn'\"'\"'t open? Use the url below\\r\\n'\nprintf '\\033]8;id=1;https://claude.com/cai/oauth/authorize?state=s1\\007https://claude.com/cai/oauth/authorize?state=s1\\r\\n'\nprintf 'Paste code here > '\nread code\nif [ \"$code\" = \"good-code\" ]; then printf 'Your OAuth token (valid for 1 year):\\r\\n{token}\\r\\n'; sleep 30; else printf 'Invalid code\\r\\n'; exit 1; fi\n"
        );
        std::fs::write(&p, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn wait_state(want: impl Fn(&View) -> bool, secs: u64) -> View {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let v = server_view();
            if want(&v) || Instant::now() > end {
                return v;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn the_server_flow_takes_the_token_stores_it_and_never_shows_it() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let kit = crate::tenant::testkit::env_once();
        crate::tenant::test_multi(true);
        let dir = kit.join("fakebin-ok");
        std::fs::create_dir_all(&dir).unwrap();
        const TOKEN: &str = "sk-ant-oat01-ServerFlowToken0123456789abcdefghijkl";
        let bin = fake_claude(&dir, TOKEN);
        std::env::set_var("ZOAI_CLAUDE_BIN", &bin);
        let t = crate::tenant::testkit::fresh(false);
        crate::tenant::scope(&t, || {
            let v = server_start().unwrap();
            assert!(matches!(v.state, State::Starting | State::Link));
            let v = wait_state(|v| v.state == State::Link, 10);
            assert_eq!(v.state, State::Link, "{v:?}");
            assert_eq!(v.url.as_deref(), Some("https://claude.com/cai/oauth/authorize?state=s1"));
            // the same person starting again gets the same flow, not a second program
            assert_eq!(running_flows(), 1);
            assert!(server_start().unwrap().url.is_some());
            assert_eq!(running_flows(), 1);
            assert!(server_submit("bad code with spaces").is_err());
            server_submit("good-code").unwrap();
            let v = wait_state(|v| v.state.over(), 10);
            assert_eq!(v.state, State::Done, "{v:?}");
            let shown = v.to_json().to_string();
            assert!(!shown.contains("ServerFlowToken") && !shown.contains("sk-ant"), "{shown}");
            assert_eq!(crate::ai::claude_code::token_source(), "own");
            // the token is in the encrypted store, and only there
            assert_eq!(crate::ai::keys::get(crate::ai::claude_code::TOKEN_KEY).as_deref(), Some(TOKEN));
            let on_disk = std::fs::read(t.config.join("keys.enc")).unwrap_or_default();
            assert!(!String::from_utf8_lossy(&on_disk).contains("ServerFlowToken"));
            server_logout().unwrap();
            assert_eq!(crate::ai::claude_code::token_source(), "none");
        });
        std::env::remove_var("ZOAI_CLAUDE_BIN");
    }

    #[test]
    fn a_wrong_code_fails_with_a_plain_message_and_a_restricted_person_is_refused_unless_all_are_allowed() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let kit = crate::tenant::testkit::env_once();
        crate::tenant::test_multi(true);
        let dir = kit.join("fakebin-bad");
        std::fs::create_dir_all(&dir).unwrap();
        let bin = fake_claude(&dir, "sk-ant-oat01-NeverPrintedToken0123456789abcdefgh");
        std::env::set_var("ZOAI_CLAUDE_BIN", &bin);
        let restricted = crate::tenant::testkit::fresh(true);
        crate::tenant::scope(&restricted, || {
            assert!(server_start().is_err(), "admins only by default");
            crate::ai::claude_code::test_users_all(Some(true));
            let v = server_start().unwrap();
            assert!(matches!(v.state, State::Starting | State::Link));
            wait_state(|v| v.state == State::Link, 10);
            server_submit("wrong").unwrap();
            let v = wait_state(|v| v.state.over(), 10);
            crate::ai::claude_code::test_users_all(None);
            assert_eq!(v.state, State::Failed, "{v:?}");
            assert!(!v.message.contains("sk-ant"), "{}", v.message);
            assert_eq!(crate::ai::claude_code::token_source(), "none");
        });
        std::env::remove_var("ZOAI_CLAUDE_BIN");
    }

    #[test]
    fn a_flow_that_is_cancelled_ends_its_program() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let kit = crate::tenant::testkit::env_once();
        crate::tenant::test_multi(true);
        let dir = kit.join("fakebin-cancel");
        std::fs::create_dir_all(&dir).unwrap();
        let bin = fake_claude(&dir, "sk-ant-oat01-NeverPrintedToken0123456789abcdefgh");
        std::env::set_var("ZOAI_CLAUDE_BIN", &bin);
        let t = crate::tenant::testkit::fresh(false);
        crate::tenant::scope(&t, || {
            server_start().unwrap();
            wait_state(|v| v.state == State::Link, 10);
            let pid = tenant_flow().unwrap().pty.pid();
            server_cancel();
            let v = wait_state(|v| v.state.over(), 5);
            assert_eq!(v.state, State::Cancelled);
            std::thread::sleep(Duration::from_millis(300));
            let alive = std::fs::read_to_string(format!("/proc/{pid}/stat")).map(|s| !s.contains(") Z")).unwrap_or(false);
            assert!(!alive, "the program is gone");
        });
        std::env::remove_var("ZOAI_CLAUDE_BIN");
    }

    /// Manual check against the real program (needs `claude` installed; signs nobody in): `cargo test -p zo-core real_claude -- --ignored`.
    #[test]
    #[ignore]
    fn real_claude_setup_token_prints_a_link_we_can_read() {
        let bin = crate::ai::claude_code::binary_path_for_manual_test().expect("claude is installed");
        let home = std::env::temp_dir().join(format!("zo-real-claude-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let mut spec = Spec::new(bin);
        spec.args = vec!["setup-token".into()];
        spec.clear_env = true;
        spec.cols = 1000;
        spec.env = vec![("PATH".into(), std::env::var("PATH").unwrap()), ("HOME".into(), home.to_string_lossy().into_owned()), ("TERM".into(), "xterm-256color".into()), ("BROWSER".into(), "/bin/true".into()), ("DISABLE_AUTOUPDATER".into(), "1".into())];
        let p = Pty::spawn(&spec).unwrap();
        let end = Instant::now() + Duration::from_secs(25);
        let mut url = None;
        while Instant::now() < end && url.is_none() {
            std::thread::sleep(Duration::from_millis(300));
            url = find_auth_url(&p.snapshot_text());
        }
        p.close();
        let _ = std::fs::remove_dir_all(&home);
        let url = url.expect("a link");
        assert!(url.starts_with("https://claude.com/") || url.starts_with("https://claude.ai/"), "{url}");
        assert!(!url.contains('\x1b') && !url.contains('\x07'));
    }
}
