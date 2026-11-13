//! The desktop link: lets the person's own web session run on THIS computer ("My computer" in the web app). See docs/LINK.md.
//!
//! Off by default. When the person turns on "Control this computer from the web" (and is signed in), a background thread keeps ONE
//! outbound long poll to the ZO-AI server (`POST /api/link/poll`, authenticated with the device credential). The server hands out
//! jobs; this computer never listens on a network port. Nothing at all goes over the network while the switch is off or nobody is
//! signed in.
//!
//! **Jobs come from the network, so they are untrusted input.** Every job is validated (type, sizes, characters, age) before it
//! does anything; unknown types and operations outside the whitelist are refused; a job older than two minutes is never run; the
//! request given to the local service is BUILT from the validated fields (a job cannot smuggle in a tenant, a mode or any other
//! key); the permission mode is capped at Auto (Bypass only when the person allowed it here); approvals are asked ON THIS
//! COMPUTER unless the person allowed answering from the web.
//!
//! A `send` job is run through this app's own service socket, exactly like the phone does it, and the events are relayed back in
//! batches (`POST /api/link/events`). The chat is saved here as usual and the sync carries it to the web.
//!
//! Also here: sending a Claude Code token (from `claude setup-token`) to the person's own server account, for the owner who wants
//! Claude Code on the server (`/api/sync/claude_token`). The token is only ever held in a `Secret`, which is wiped when dropped,
//! and it is never logged, saved, or put in an error.

use crate::policy::Mode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A job that is older than this when it reaches us is never run.
pub const MAX_JOB_AGE_MS: u64 = 120_000;
/// A job runs for at most this long.
pub const MAX_JOB_SECS: u64 = 3600;
/// What one job may send back in total.
pub const MAX_JOB_BYTES: usize = 50 * 1024 * 1024;
/// One batch of events.
pub const MAX_BATCH_BYTES: usize = 1024 * 1024;
const MAX_TEXT_CHARS: usize = 100_000;
const MAX_ATTACH_BYTES: usize = 8 * 1024 * 1024;
const APPROVAL_WAIT: Duration = Duration::from_secs(180);

/// The operations the web may ask of this computer (the model pill and the profile menu), and nothing else.
pub const OPS: &[&str] = &["status", "providers", "profiles", "profile_use", "models", "select", "mode"];

// ---------------------------------------------------------------------------------------------------------------------------
// settings (~/.config/zo-ai/link.json)

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// "Control this computer from the web". Default off.
    pub enabled: bool,
    /// Answer approvals from the web instead of on this computer. Default off.
    pub approvals_from_web: bool,
    /// Let a web job run in Bypass mode. Default off: Auto is the most a web job gets.
    pub allow_bypass: bool,
    /// The name this computer shows in the web app.
    pub name: String,
    /// When the Claude Code token was last sent to the server (ms since 1970; 0 = never). Not a secret.
    pub claude_sent_at: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: false, approvals_from_web: false, allow_bypass: false, name: String::new(), claude_sent_at: 0 }
    }
}

fn config_path() -> PathBuf {
    crate::audit::config_dir().join("link.json")
}

pub fn config() -> Config {
    let v: Value = std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    Config {
        enabled: b("enabled"),
        approvals_from_web: b("approvals_from_web"),
        allow_bypass: b("allow_bypass"),
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        claude_sent_at: v.get("claude_sent_at").and_then(Value::as_u64).unwrap_or(0),
    }
}

pub fn save_config(c: &Config) -> Result<(), String> {
    let name = if c.name.trim().is_empty() { String::new() } else { crate::login::clean_device_name(&c.name)? };
    let p = config_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let v = json!({"enabled": c.enabled, "approvals_from_web": c.approvals_from_web, "allow_bypass": c.allow_bypass, "name": name, "claude_sent_at": c.claude_sent_at});
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    kick();
    Ok(())
}

/// Changes one setting without touching the others.
pub fn update_config(f: impl FnOnce(&mut Config)) -> Result<(), String> {
    let mut c = config();
    f(&mut c);
    save_config(&c)
}

/// The name shown in the web app.
pub fn device_name() -> String {
    let n = config().name;
    if n.trim().is_empty() { crate::login::default_device_name("desktop") } else { n }
}

static WAKE: AtomicBool = AtomicBool::new(false);
fn kick() {
    WAKE.store(true, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------------------------------------------------------
// the permission cap

/// The mode a web job runs in: what it asked for if that is as strict as or stricter than the mode saved on this computer (else the
/// saved mode: the web cannot loosen the protection the person set), and never Bypass unless the person allowed that on this
/// computer. A job that wants Bypass gets Auto.
pub fn cap_mode(requested: Option<Mode>, saved: Mode, allow_bypass: bool) -> Mode {
    let m = match requested {
        Some(r) if strength(r) < strength(saved) => r,
        // looser than saved only by the person's own opt-in "Allow Bypass mode from the web"
        Some(Mode::Bypass) if allow_bypass => Mode::Bypass,
        _ => saved,
    };
    match m {
        Mode::Bypass if !allow_bypass => Mode::Auto,
        m => m,
    }
}

fn strength(m: Mode) -> u8 {
    match m {
        Mode::Manual => 0,
        Mode::Auto => 1,
        Mode::Bypass => 2,
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// jobs (untrusted)

#[derive(Debug, PartialEq)]
pub enum Kind {
    Send { text: String, chat: Option<String>, edit: Option<u64>, attachments: Option<Value>, mode: Option<Mode> },
    Op { op: String, args: Vec<(String, Value)> },
    Stop { job_id: Option<String> },
    /// A terminal on this computer for the web page. Opened only after the person here says yes (never from the web).
    Term { rows: u16, cols: u16 },
    ApprovalReply { job_id: String, approval: u64, yes: bool },
    /// The person's answer to a question card (`zohara_ask_user`) of a running web session: `reply` is the checked
    /// `{"cancelled":true}` or `{"answers":[...]}`; the service checks it again against the question it asked.
    AnswerReply { job_id: String, question: u64, reply: Value },
}

#[derive(Debug, PartialEq)]
pub struct Job {
    pub id: String,
    pub kind: Kind,
}

fn safe_id(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

/// How old a job is when it arrives: the server's `age_ms` (computed at delivery), else the time since its `created_ms`
/// (this computer's clock against the server's, so only used when there is no age). Unknown counts as fresh: the server
/// itself drops jobs that wait more than two minutes.
fn age_ms(v: &Value, now_ms: u64) -> u64 {
    if let Some(a) = v.get("age_ms").and_then(Value::as_u64) {
        return a;
    }
    match v.get("created_ms").and_then(Value::as_u64) {
        Some(c) => now_ms.saturating_sub(c),
        None => 0,
    }
}

/// Checks one job from the server. Err is (the job's id when it has a usable one, why it is refused).
pub fn parse_job(v: &Value, now_ms: u64) -> Result<Job, (Option<String>, String)> {
    let id = v.get("id").and_then(Value::as_str).unwrap_or("");
    if !safe_id(id, 64) {
        return Err((None, "A job without a usable id.".into()));
    }
    let id = id.to_string();
    let bad = |why: &str| Err((Some(id.clone()), why.to_string()));
    if age_ms(v, now_ms) >= MAX_JOB_AGE_MS {
        return bad("This job waited too long and was not run.");
    }
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    let kind = match ty {
        "send" => {
            let text = v.get("text").and_then(Value::as_str).unwrap_or("");
            let edit = v.get("edit").and_then(Value::as_u64);
            if text.trim().is_empty() && edit.is_none() {
                return bad("The message is empty.");
            }
            if text.chars().count() > MAX_TEXT_CHARS {
                return bad("The message is too long.");
            }
            let chat = match v.get("chat") {
                None | Some(Value::Null) => None,
                Some(Value::String(c)) if c.is_empty() => None,
                Some(Value::String(c)) if safe_id(c, 64) => Some(c.clone()),
                _ => return bad("The chat id is not valid."),
            };
            let attachments = match v.get("attachments") {
                None | Some(Value::Null) => None,
                Some(a @ Value::Array(items)) if items.len() <= 20 && a.to_string().len() <= MAX_ATTACH_BYTES => Some(a.clone()),
                _ => return bad("The attachments are not valid or too big."),
            };
            let mode = match v.get("mode") {
                None | Some(Value::Null) => None,
                Some(Value::String(m)) => match Mode::parse_known(m) {
                    Some(m) => Some(m),
                    None => return bad("Unknown permission mode."),
                },
                _ => return bad("Unknown permission mode."),
            };
            Kind::Send { text: text.to_string(), chat, edit, attachments, mode }
        }
        "op" => {
            let inner = v.get("inner").or_else(|| v.get("op_args"));
            let (name, obj) = match (v.get("op").and_then(Value::as_str), inner) {
                (Some(n), Some(Value::Object(o))) => (n, Some(o)),
                (Some(n), None) => (n, None),
                (None, Some(Value::Object(o))) => (o.get("op").and_then(Value::as_str).unwrap_or(""), Some(o)),
                _ => ("", None),
            };
            let control = crate::control::OPS.contains(&name);
            if !OPS.contains(&name) && !control {
                return bad("This operation is not allowed from the web.");
            }
            let mut args = Vec::new();
            if let (Some(o), true) = (obj, control) {
                // the control operations take one argument, a path, checked again when it is resolved
                match o.get("path") {
                    None | Some(Value::Null) => {}
                    Some(Value::String(s)) if s.chars().count() <= crate::control::MAX_PATH_CHARS && !s.chars().any(char::is_control) => args.push(("path".to_string(), Value::String(s.clone()))),
                    _ => return bad("The path is not valid."),
                }
            } else if let Some(o) = obj {
                for key in ["id", "provider", "model", "effort", "set"] {
                    match o.get(key) {
                        None | Some(Value::Null) => {}
                        Some(Value::String(s)) if s.chars().count() <= 200 && !s.chars().any(char::is_control) => args.push((key.to_string(), Value::String(s.clone()))),
                        _ => return bad("An argument is not valid."),
                    }
                }
            }
            Kind::Op { op: name.to_string(), args }
        }
        "term" => {
            let dim = |k: &str, lo: u64, hi: u64, d: u64| match v.get(k) {
                None | Some(Value::Null) => Some(d),
                Some(n) => n.as_u64().filter(|n| (lo..=hi).contains(n)),
            };
            match (dim("rows", 2, 500, 30), dim("cols", 10, 2000, 110)) {
                (Some(r), Some(c)) => Kind::Term { rows: r as u16, cols: c as u16 },
                _ => return bad("The terminal size is not valid."),
            }
        }
        "stop" => match v.get("job_id").or_else(|| v.get("target")) {
            None | Some(Value::Null) => Kind::Stop { job_id: None },
            Some(Value::String(s)) if safe_id(s, 64) => Kind::Stop { job_id: Some(s.clone()) },
            _ => return bad("The job to stop is not valid."),
        },
        "approval_reply" => {
            let job = v.get("job_id").and_then(Value::as_str).unwrap_or("");
            let approval = v.get("approval").or_else(|| v.get("approval_id")).and_then(Value::as_u64);
            let yes = v.get("yes").and_then(Value::as_bool);
            match (safe_id(job, 64), approval, yes) {
                (true, Some(a), Some(y)) => Kind::ApprovalReply { job_id: job.to_string(), approval: a, yes: y },
                _ => return bad("The approval answer is not valid."),
            }
        }
        "answer_reply" => {
            let job = v.get("job_id").and_then(Value::as_str).unwrap_or("");
            let qid = v.get("question_id").or_else(|| v.get("question")).and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())));
            let reply = if v.get("cancelled").and_then(Value::as_bool) == Some(true) {
                Some(json!({"cancelled": true}))
            } else {
                clean_answers(v.get("answers")).map(|a| json!({"answers": a}))
            };
            match (safe_id(job, 64), qid, reply) {
                (true, Some(q), Some(r)) => Kind::AnswerReply { job_id: job.to_string(), question: q, reply: r },
                _ => return bad("The answer to the question is not valid."),
            }
        }
        _ => return bad("Unknown job type."),
    };
    Ok(Job { id, kind })
}

/// The `answers` of an answer job rebuilt from checked fields only: at most 4 answers, each `{selected: [<=6 texts of <=100
/// characters], other: <=4000 characters or null}`. (What the answers MEAN is checked by the service against its own question.)
fn clean_answers(v: Option<&Value>) -> Option<Value> {
    let list = v?.as_array().filter(|l| !l.is_empty() && l.len() <= 4)?;
    let mut out = Vec::new();
    for a in list {
        let sel = match a.get("selected") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) if items.len() <= 6 => items.iter().map(|i| i.as_str().filter(|s| s.chars().count() <= 100).map(|s| json!(s))).collect::<Option<Vec<_>>>()?,
            _ => return None,
        };
        let other = match a.get("other") {
            None | Some(Value::Null) => Value::Null,
            Some(Value::String(s)) if s.chars().count() <= 4000 => json!(s),
            _ => return None,
        };
        out.push(json!({"selected": sel, "other": other}));
    }
    Some(Value::Array(out))
}

/// The `op` request for the local service, built from validated arguments only. The mode operation can only make this computer
/// stricter: it never raises the saved mode (and never to Bypass), so the web cannot loosen the protection the person set.
pub fn op_request(op: &str, args: &[(String, Value)], saved: Mode) -> Result<Value, String> {
    if !OPS.contains(&op) {
        return Err("This operation is not allowed from the web.".into());
    }
    let mut req = serde_json::Map::new();
    req.insert("op".into(), json!(op));
    req.insert("client".into(), json!("web-link"));
    for (k, v) in args {
        req.insert(k.clone(), v.clone());
    }
    if op == "mode" {
        if let Some(Value::String(want)) = req.get("set") {
            match Mode::parse_known(want) {
                Some(m) if strength(m) <= strength(saved) => {}
                _ => return Err("The web can only make this computer stricter, not looser. Change the mode on the computer.".into()),
            }
        }
    }
    Ok(Value::Object(req))
}

/// The request for the local service for a `send` job.
pub fn send_request(text: &str, chat: &Option<String>, edit: Option<u64>, attachments: &Option<Value>, mode: Mode) -> Value {
    // "can_ask": the web page shows question cards, so the model may ask (the answer comes back as an `answer_reply` job)
    let mut r = json!({"op": "send", "text": text, "client": "web-link", "mode": mode.name().to_ascii_lowercase(), "can_ask": true});
    if let Some(c) = chat {
        r["chat"] = json!(c);
    }
    if let Some(e) = edit {
        r["edit"] = json!(e);
    }
    if let Some(a) = attachments {
        r["attachments"] = a.clone();
    }
    r
}

// ---------------------------------------------------------------------------------------------------------------------------
// what the app provides

/// What the program that hosts the link (the desktop app) provides.
pub trait Host: Send + Sync {
    /// Asks the person ON THIS COMPUTER (a dialog); blocks until answered. False for no, and when nobody answers.
    fn ask_local(&self, summary: &str) -> bool;
}

/// Where the server is and who this computer is, or None when the link must not run now (off, or nobody signed in).
pub type Endpoint = Arc<dyn Fn() -> Option<(String, String)> + Send + Sync>;
/// Connects to this app's own service.
pub type Connect = Arc<dyn Fn() -> std::io::Result<UnixStream> + Send + Sync>;

/// The production endpoint: the switch is on, someone is signed in, the server address is acceptable.
pub fn default_endpoint() -> Endpoint {
    Arc::new(|| {
        if !config().enabled {
            return None;
        }
        let cred = crate::login::credential_of(crate::login::Client::Desktop)?;
        if crate::login::email_of(crate::login::Client::Desktop).is_empty() {
            return None;
        }
        Some((crate::login::server_url().ok()?, cred))
    })
}

pub fn default_connect() -> Connect {
    Arc::new(|| UnixStream::connect(crate::service::socket_path()))
}

// ---------------------------------------------------------------------------------------------------------------------------
// state, for the screen

#[derive(Clone, Default, Debug)]
pub struct State {
    /// The last poll reached the server.
    pub connected: bool,
    pub last_ok_ms: u64,
    pub last_error: String,
    /// The job being run (a web session), if any.
    pub active: Option<String>,
    pub jobs_done: u64,
}

static STATE: Mutex<State> = Mutex::new(State { connected: false, last_ok_ms: 0, last_error: String::new(), active: None, jobs_done: 0 });

fn st() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn state() -> State {
    st().clone()
}

fn now_ms() -> u64 {
    crate::activity::now_ms()
}

// ---------------------------------------------------------------------------------------------------------------------------
// talking to the server

struct Failure {
    status: u16,
    retry_after: u64,
    message: String,
}

fn post(url: &str, cred: &str, body: &Value, timeout: Duration) -> Result<Value, Failure> {
    let fail = |status: u16, message: String| Failure { status, retry_after: 0, message };
    match ureq::post(url).timeout(timeout).set("Authorization", &format!("Bearer {cred}")).send_json(body.clone()) {
        Ok(r) => r.into_json::<Value>().map_err(|_| fail(0, "The server's answer was not understood.".into())),
        Err(ureq::Error::Status(429, r)) => Err(Failure { status: 429, retry_after: crate::sync::retry_after_secs(r.header("Retry-After")), message: "Too many requests.".into() }),
        Err(ureq::Error::Status(c, r)) => {
            let m = r.into_json::<Value>().ok().and_then(|v| v.get("error").and_then(Value::as_str).map(|s| s.chars().take(300).collect::<String>()));
            Err(fail(c, m.unwrap_or_else(|| format!("The server answered {c}."))))
        }
        Err(ureq::Error::Transport(t)) => Err(fail(0, format!("Cannot reach the server: {}", t.kind()))),
    }
}

fn caps() -> Value {
    let cc = crate::ai::claude_code::binary().is_some();
    json!({"claude_code": cc, "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "approvals_from_web": config().approvals_from_web})
}

/// The seconds to wait after the n-th failure in a row (2, 4, 8 ... 60).
pub fn backoff_secs(failures: u32) -> u64 {
    (1u64 << failures.clamp(1, 6)).min(60)
}

// ---------------------------------------------------------------------------------------------------------------------------
// running jobs

/// What a running job shares with the poll loop.
struct Ctl {
    stop: AtomicBool,
    /// Approvals this job waits for (answered from the web), by approval id.
    waiting: Mutex<HashMap<u64, mpsc::Sender<bool>>>,
    approvals_from_web: bool,
    /// Question cards that were shown to the web for this job (ids announced by the service): only these can be answered.
    questions: Mutex<std::collections::HashSet<u64>>,
    /// The connection to the service that runs this job, for forwarding an answer.
    service: Mutex<Option<Arc<Mutex<UnixStream>>>>,
}

struct Shared {
    host: Arc<dyn Host>,
    connect: Connect,
    jobs: Mutex<HashMap<String, Arc<Ctl>>>,
    /// Terminals open for the web (not in `jobs`: a terminal does not make this computer "busy" for a chat).
    terms: Mutex<HashMap<String, Arc<Ctl>>>,
    /// Counter of web jobs run, so a test can wait for them.
    finished: AtomicU64,
}

fn report(base: &str, cred: &str, job_id: &str, events: Vec<Value>, done: bool) -> Result<bool, Failure> {
    let r = post(&format!("{base}/api/link/events"), cred, &json!({"job_id": job_id, "events": events, "done": done}), Duration::from_secs(20))?;
    Ok(r.get("cancel").and_then(Value::as_bool).unwrap_or(false))
}

/// An event that is too big is cut (tool output and text are the only big things).
fn trim_event(mut e: Value) -> Value {
    if e.to_string().len() > 100_000 {
        for k in ["text", "output", "summary"] {
            if let Some(s) = e.get(k).and_then(Value::as_str).map(|s| s.chars().take(40_000).collect::<String>()) {
                e[k] = json!(s);
            }
        }
    }
    e
}

/// Sends a batch, retrying a few times on a failure. Err means the server is gone or refuses the job.
fn flush(base: &str, cred: &str, job_id: &str, batch: &mut Vec<Value>, done: bool) -> Result<bool, ()> {
    if batch.is_empty() && !done {
        return Ok(false);
    }
    let mut tries = 0;
    loop {
        match report(base, cred, job_id, batch.clone(), done) {
            Ok(c) => {
                batch.clear();
                return Ok(c);
            }
            // the server does not know the job any more: it expired, or was stopped there
            Err(f) if matches!(f.status, 400 | 403 | 404 | 409 | 410) => return Err(()),
            Err(f) => {
                tries += 1;
                if tries >= 4 {
                    return Err(());
                }
                std::thread::sleep(Duration::from_secs(if f.status == 429 { f.retry_after.clamp(1, 10) } else { tries as u64 }));
            }
        }
    }
}

fn run_send(sh: Arc<Shared>, base: String, cred: String, id: String, ctl: Arc<Ctl>, req: Value) {
    let conn = match (sh.connect)() {
        Ok(c) => c,
        Err(_) => {
            let mut b = vec![json!({"event": "error", "error": "ZO-AI on this computer did not answer. Restart it."})];
            let _ = flush(&base, &cred, &id, &mut b, true);
            return;
        }
    };
    let Ok(read_half) = conn.try_clone() else { return };
    let out = Arc::new(Mutex::new(conn));
    *ctl.service.lock().unwrap_or_else(|e| e.into_inner()) = Some(out.clone());
    let say = |v: &Value| -> bool {
        let mut s = v.to_string();
        s.push('\n');
        out.lock().map(|mut o| o.write_all(s.as_bytes()).is_ok()).unwrap_or(false)
    };
    if !say(&req) {
        return;
    }
    // lines from the service arrive on a thread of their own, so this loop can also watch the clock and the cancel flags
    let (tx, rx) = mpsc::channel::<Option<Value>>();
    std::thread::spawn(move || {
        for l in BufReader::new(read_half).lines() {
            let Ok(l) = l else { break };
            if let Ok(v) = serde_json::from_str::<Value>(&l) {
                if tx.send(Some(v)).is_err() {
                    return;
                }
            }
        }
        let _ = tx.send(None);
    });
    let started = Instant::now();
    let (mut batch, mut batch_bytes, mut total, mut first_at) = (Vec::<Value>::new(), 0usize, 0usize, None::<Instant>);
    let mut stop_sent = false;
    let mut finished = false;
    let mut gone = false;
    loop {
        let stop_now = ctl.stop.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(MAX_JOB_SECS) || gone;
        if stop_now && !stop_sent {
            stop_sent = true;
            say(&json!({"op": "stop"}));
        }
        let msg = rx.recv_timeout(Duration::from_millis(150));
        let mut flush_now = false;
        match msg {
            Ok(Some(ev)) => {
                let kind = ev.get("event").and_then(Value::as_str).unwrap_or("").to_string();
                if kind == "approval" {
                    let aid = ev.get("id").and_then(Value::as_u64).unwrap_or(u64::MAX);
                    let summary: String = ev.get("summary").and_then(Value::as_str).unwrap_or("").chars().take(2000).collect();
                    if ctl.approvals_from_web {
                        let (atx, arx) = mpsc::channel();
                        ctl.waiting.lock().unwrap_or_else(|e| e.into_inner()).insert(aid, atx);
                        let (out2, ctl2) = (out.clone(), ctl.clone());
                        std::thread::spawn(move || {
                            let yes = arx.recv_timeout(APPROVAL_WAIT).unwrap_or(false);
                            ctl2.waiting.lock().unwrap_or_else(|e| e.into_inner()).remove(&aid);
                            let mut s = json!({"op": "approve", "id": aid, "yes": yes}).to_string();
                            s.push('\n');
                            let _ = out2.lock().map(|mut o| o.write_all(s.as_bytes()));
                        });
                        let mut e2 = ev.clone();
                        e2["local"] = json!(false);
                        batch.push(trim_event(e2));
                    } else {
                        // asked here, on this computer; the web only hears that it waits
                        let (out2, host) = (out.clone(), sh.host.clone());
                        let s2 = summary.clone();
                        std::thread::spawn(move || {
                            let yes = host.ask_local(&s2);
                            let mut s = json!({"op": "approve", "id": aid, "yes": yes}).to_string();
                            s.push('\n');
                            let _ = out2.lock().map(|mut o| o.write_all(s.as_bytes()));
                        });
                        batch.push(json!({"event": "approval", "id": aid, "tool": ev.get("tool").cloned().unwrap_or(Value::Null), "summary": summary, "local": true}));
                    }
                    flush_now = true;
                } else {
                    // a question card goes to the web as it is; its answer comes back as an `answer_reply` job (only for this id)
                    if kind == "question" {
                        if let Some(q) = ev.get("id").and_then(Value::as_u64) {
                            let mut open = ctl.questions.lock().unwrap_or_else(|e| e.into_inner());
                            if open.len() < 16 {
                                open.insert(q);
                            }
                        }
                    }
                    batch.push(trim_event(ev));
                    flush_now = matches!(kind.as_str(), "error" | "stopped" | "done" | "chat" | "question");
                }
                if kind == "done" {
                    finished = true;
                }
                if let Some(e) = batch.last() {
                    batch_bytes += e.to_string().len();
                }
                first_at.get_or_insert_with(Instant::now);
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => finished = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        total += std::mem::take(&mut batch_bytes).min(MAX_BATCH_BYTES * 4);
        // batches are never bigger than the limit
        if batch.iter().map(|e| e.to_string().len()).sum::<usize>() >= MAX_BATCH_BYTES / 2 || first_at.map(|t| t.elapsed() > Duration::from_millis(250)).unwrap_or(false) {
            flush_now = true;
        }
        if total > MAX_JOB_BYTES && !stop_sent {
            stop_sent = true;
            say(&json!({"op": "stop"}));
            batch.push(json!({"event": "error", "error": "This answer got too big and was stopped."}));
        }
        if flush_now || finished {
            first_at = None;
            match flush(&base, &cred, &id, &mut batch, finished) {
                Ok(cancel) => {
                    if cancel {
                        ctl.stop.store(true, Ordering::SeqCst);
                    }
                }
                Err(()) => {
                    // the server is gone or does not want this job: stop it here too, and stop reporting
                    gone = true;
                    ctl.stop.store(true, Ordering::SeqCst);
                    if finished {
                        break;
                    }
                    batch.clear();
                }
            }
        }
        if finished {
            break;
        }
    }
    ctl.questions.lock().unwrap_or_else(|e| e.into_inner()).clear();
    *ctl.service.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let _ = out.lock().map(|o| o.shutdown(std::net::Shutdown::Both));
    // the chat was saved on this computer: let the sync carry it to the web now
    crate::sync::kick();
}

fn run_op(sh: &Shared, base: &str, cred: &str, id: &str, op: &str, args: &[(String, Value)]) {
    let saved = Mode::load();
    let mut events = Vec::new();
    match op_request(op, args, saved) {
        Err(e) => events.push(json!({"event": "error", "error": e})),
        Ok(req) => match (sh.connect)() {
            Err(_) => events.push(json!({"event": "error", "error": "ZO-AI on this computer did not answer. Restart it."})),
            Ok(conn) => {
                let _ = conn.set_read_timeout(Some(Duration::from_secs(60)));
                let mut w = match conn.try_clone() {
                    Ok(w) => w,
                    Err(_) => return,
                };
                let mut line = req.to_string();
                line.push('\n');
                let mut reply = String::new();
                if w.write_all(line.as_bytes()).is_ok() && BufReader::new(conn).read_line(&mut reply).unwrap_or(0) > 0 {
                    match serde_json::from_str::<Value>(&reply) {
                        Ok(v) => events.push(json!({"event": "op_result", "reply": v})),
                        Err(_) => events.push(json!({"event": "error", "error": "The answer was not understood."})),
                    }
                } else {
                    events.push(json!({"event": "error", "error": "ZO-AI on this computer did not answer."}));
                }
            }
        },
    }
    let _ = flush(base, cred, id, &mut events, true);
}

/// Terminals one computer keeps open for the web at once.
pub const MAX_TERMS: usize = 3;
/// What the person here is told when a web page asks for a terminal. It is asked on this computer every time, and no setting lets the
/// web answer it.
pub const TERM_QUESTION: &str = "open a TERMINAL on this computer. It can run any command as you, the same as a terminal you opened yourself. Allow it only if you just asked for it in the web page";
/// The smallest time between two posts of a terminal's output (the server takes 600 posts a minute).
const TERM_POST_GAP: Duration = Duration::from_millis(120);

fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// A terminal for the web page: asks the person on this computer, then runs a login shell of theirs and relays it until the page
/// closes it, the link goes away or the shell ends. The shell is the person's own program in their own home; what the web may do is
/// type into it and resize it.
fn run_term(sh: &Shared, base: &str, cred: &str, id: &str, ctl: &Ctl, rows: u16, cols: u16) {
    // the page is told that it waits; the question is on this computer's screen, whatever the settings say
    let mut b = vec![json!({"event": "waiting"})];
    if flush(base, cred, id, &mut b, false).is_err() {
        return;
    }
    let yes = sh.host.ask_local(TERM_QUESTION);
    if !yes || ctl.stop.load(Ordering::SeqCst) {
        let mut b = vec![json!({"event": "denied"})];
        let _ = flush(base, cred, id, &mut b, true);
        return;
    }
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty() && std::path::Path::new(s).is_file()).unwrap_or_else(|| "/bin/bash".into());
    let mut spec = crate::pty::Spec::new(&shell);
    spec.args = vec!["-l".into()];
    spec.rows = rows;
    spec.cols = cols;
    spec.env = vec![("TERM".into(), "xterm-256color".into()), ("COLORTERM".into(), "truecolor".into())];
    spec.cwd = std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_dir());
    let pty = match crate::pty::Pty::spawn(&spec) {
        Ok(p) => p,
        Err(e) => {
            let mut b = vec![json!({"event": "error", "error": e})];
            let _ = flush(base, cred, id, &mut b, true);
            return;
        }
    };
    let mut batch = vec![json!({"event": "opened"})];
    let (mut seq, mut last_post, mut failures) = (0u64, Instant::now() - TERM_POST_GAP, 0u32);
    let mut ended = false;
    loop {
        // wait for output, but never longer than a post gap: typed input comes back in the answer to a post
        let wait = TERM_POST_GAP.saturating_sub(last_post.elapsed()).max(Duration::from_millis(10));
        let mut c = pty.read_from(seq, wait);
        let mut chunks = 0;
        while !c.data.is_empty() && chunks < 8 {
            batch.push(json!({"event": "out", "b": b64_encode(&c.data)}));
            seq = c.next;
            chunks += 1;
            c = pty.read_from(seq, Duration::ZERO);
        }
        if c.ended && c.data.is_empty() {
            ended = true;
            batch.push(json!({"event": "exit", "code": pty.exit_code().unwrap_or(-1)}));
        }
        let due = !batch.is_empty() || last_post.elapsed() >= Duration::from_millis(300);
        if due && last_post.elapsed() >= TERM_POST_GAP || ended || ctl.stop.load(Ordering::SeqCst) {
            let stopping = ctl.stop.load(Ordering::SeqCst);
            let done = ended || stopping;
            match post(&format!("{base}/api/link/events"), cred, &json!({"job_id": id, "events": batch, "done": done}), Duration::from_secs(20)) {
                Ok(r) => {
                    failures = 0;
                    batch.clear();
                    last_post = Instant::now();
                    if r.get("cancel").and_then(Value::as_bool).unwrap_or(false) {
                        break;
                    }
                    for item in r.get("term").and_then(Value::as_array).into_iter().flatten().take(64) {
                        match item.get("t").and_then(Value::as_str) {
                            Some("in") => {
                                if let Some(d) = item.get("data").and_then(Value::as_str).filter(|d| d.len() <= crate::pty::MAX_INPUT) {
                                    let _ = pty.write(d.as_bytes());
                                }
                            }
                            Some("resize") => {
                                let g = |k: &str| item.get(k).and_then(Value::as_u64).filter(|n| *n >= 2 && *n <= 2000);
                                if let (Some(r), Some(c)) = (g("rows"), g("cols")) {
                                    pty.resize(r as u16, c as u16);
                                }
                            }
                            Some("close") => ctl.stop.store(true, Ordering::SeqCst),
                            _ => {}
                        }
                    }
                }
                // the server does not know this terminal (any more): it is over
                Err(f) if matches!(f.status, 400 | 403 | 404 | 409 | 410) => break,
                Err(_) => {
                    failures += 1;
                    // the link is gone: the terminal closes (what was typed meanwhile is not lost, but nothing runs unwatched for long)
                    if failures >= 20 {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            if done {
                break;
            }
        }
    }
    pty.close();
}

/// How long a control request may wait for the person's answer on this computer. The server gives up on the page's question a
/// little later than this, so an answer that comes after it is never acted on ("never run late").
pub const CONTROL_WAIT: Duration = Duration::from_secs(90);

/// One control request (`sys_info`, `fs_list`, `fs_read`, `screenshot`; docs/FULL-CONTROL.md phase 1) at a time: a second one while a
/// dialog is open is refused, so the person never faces a stack of dialogs.
static CONTROL_BUSY: AtomicBool = AtomicBool::new(false);

struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        CONTROL_BUSY.store(false, Ordering::SeqCst);
    }
}

/// What the dialog on this computer says about a control request (built from the RESOLVED path, never from the web's text).
fn control_summary(op: &str, target: &str) -> String {
    match op {
        "fs_list" => format!("list the files in {target}"),
        "fs_read" => format!("read the file {target}"),
        "screenshot" => "take a picture of your screen and send it to the web page".to_string(),
        _ => op.to_string(),
    }
}

/// Runs a control request: the path is resolved and checked first (a refused path never shows a dialog), then the person is asked
/// ON THIS COMPUTER when the operation shows personal content, then the thing is done. Every request, answered or refused, is
/// written to the audit log.
fn run_control(sh: &Shared, base: &str, cred: &str, id: &str, op: &str, args: &[(String, Value)], received: Instant) {
    let reply = control_reply(&*sh.host, op, args, received);
    let mut events = vec![match &reply {
        Ok(v) => json!({"event": "op_result", "reply": v}),
        Err(e) => json!({"event": "error", "error": e}),
    }];
    let _ = flush(base, cred, id, &mut events, true);
}

fn control_reply(host: &dyn Host, op: &str, args: &[(String, Value)], received: Instant) -> Result<Value, String> {
    use crate::control as c;
    let path_arg = args.iter().find(|(k, _)| k == "path").and_then(|(_, v)| v.as_str()).unwrap_or("");
    let audit = |outcome: &str, shown: &str| crate::audit::log(&format!("web:{op}"), &json!({"path": shown}), outcome);
    if CONTROL_BUSY.swap(true, Ordering::SeqCst) {
        return Err("This computer is already waiting for your answer to another web request.".into());
    }
    let _busy = BusyGuard;
    // 1. what it would touch, checked before anyone is asked
    let target = match op {
        "fs_list" | "fs_read" => match c::resolve(path_arg) {
            Ok(p) => Some(p),
            Err(e) => {
                audit("refused", &c::clean(path_arg, 200));
                return Err(e);
            }
        },
        _ => None,
    };
    let shown = target.as_deref().map(c::shown).unwrap_or_default();
    // 2. the person, on this computer
    if c::needs_approval(op) {
        let yes = host.ask_local(&control_summary(op, &shown));
        if !yes {
            audit("denied on the computer", &shown);
            return Err("You said no on your computer.".into());
        }
        if received.elapsed() >= CONTROL_WAIT {
            audit("too late", &shown);
            return Err("The answer came too late: the web page had given up. Ask again.".into());
        }
    }
    // 3. the thing itself (read-only)
    let r = match (op, target) {
        ("sys_info", _) => c::sys_info(),
        ("fs_list", Some(p)) => c::list_dir(&p),
        ("fs_read", Some(p)) => c::read_text(&p),
        ("screenshot", _) => c::screenshot(),
        _ => Err("This operation is not allowed from the web.".into()),
    };
    audit(if r.is_ok() { "done" } else { "failed" }, &shown);
    r
}

fn start_job(sh: &Arc<Shared>, base: &str, cred: &str, job: Job) {
    let (base, cred) = (base.to_string(), cred.to_string());
    match job.kind {
        Kind::Send { text, chat, edit, attachments, mode } => {
            let cfg = config();
            let busy = !sh.jobs.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
            if busy {
                let mut b = vec![json!({"event": "error", "error": "This computer is already busy with another web session."})];
                std::thread::spawn(move || {
                    let _ = flush(&base, &cred, &job.id, &mut b, true);
                });
                return;
            }
            let capped = cap_mode(mode, Mode::load(), cfg.allow_bypass);
            let req = send_request(&text, &chat, edit, &attachments, capped);
            let ctl = Arc::new(Ctl { stop: AtomicBool::new(false), waiting: Mutex::new(HashMap::new()), approvals_from_web: cfg.approvals_from_web, questions: Mutex::new(Default::default()), service: Mutex::new(None) });
            sh.jobs.lock().unwrap_or_else(|e| e.into_inner()).insert(job.id.clone(), ctl.clone());
            st().active = Some(job.id.clone());
            let sh2 = sh.clone();
            std::thread::spawn(move || {
                run_send(sh2.clone(), base, cred, job.id.clone(), ctl, req);
                sh2.jobs.lock().unwrap_or_else(|e| e.into_inner()).remove(&job.id);
                let mut s = st();
                if s.active.as_deref() == Some(job.id.as_str()) {
                    s.active = None;
                }
                s.jobs_done += 1;
                sh2.finished.fetch_add(1, Ordering::SeqCst);
            });
        }
        Kind::Op { op, args } => {
            let sh2 = sh.clone();
            let received = Instant::now();
            std::thread::spawn(move || {
                if crate::control::OPS.contains(&op.as_str()) {
                    run_control(&sh2, &base, &cred, &job.id, &op, &args, received);
                } else {
                    run_op(&sh2, &base, &cred, &job.id, &op, &args);
                }
                sh2.finished.fetch_add(1, Ordering::SeqCst);
            });
        }
        Kind::Stop { job_id } => {
            let jobs = sh.jobs.lock().unwrap_or_else(|e| e.into_inner());
            for (id, ctl) in jobs.iter() {
                if job_id.as_deref().map(|j| j == id).unwrap_or(true) {
                    ctl.stop.store(true, Ordering::SeqCst);
                }
            }
            // a stop names the job it is for: a terminal ends only when it is named (a general "stop" is for chats)
            if let Some(j) = job_id {
                if let Some(ctl) = sh.terms.lock().unwrap_or_else(|e| e.into_inner()).get(&j) {
                    ctl.stop.store(true, Ordering::SeqCst);
                }
            }
        }
        Kind::Term { rows, cols } => {
            let open = sh.terms.lock().unwrap_or_else(|e| e.into_inner()).len();
            if open >= MAX_TERMS {
                let mut b = vec![json!({"event": "error", "error": "This computer already has as many web terminals open as it allows."})];
                std::thread::spawn(move || {
                    let _ = flush(&base, &cred, &job.id, &mut b, true);
                });
                return;
            }
            let ctl = Arc::new(Ctl { stop: AtomicBool::new(false), waiting: Mutex::new(HashMap::new()), approvals_from_web: false });
            sh.terms.lock().unwrap_or_else(|e| e.into_inner()).insert(job.id.clone(), ctl.clone());
            let sh2 = sh.clone();
            std::thread::spawn(move || {
                run_term(&sh2, &base, &cred, &job.id, &ctl, rows, cols);
                sh2.terms.lock().unwrap_or_else(|e| e.into_inner()).remove(&job.id);
                sh2.finished.fetch_add(1, Ordering::SeqCst);
            });
        }
        Kind::AnswerReply { job_id, question, reply } => {
            // only for a running job and a question card that this job showed. A question is just the person's own words (it runs
            // nothing), so it does not need the "approvals from the web" switch; the service checks the content again.
            let ctl = sh.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(&job_id).cloned();
            if let Some(ctl) = ctl {
                if ctl.questions.lock().unwrap_or_else(|e| e.into_inner()).contains(&question) {
                    let mut line = json!({"op": "answer", "id": question});
                    if let (Some(o), Some(r)) = (line.as_object_mut(), reply.as_object()) {
                        o.extend(r.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                    if let Some(svc) = ctl.service.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                        let mut s = line.to_string();
                        s.push('\n');
                        let _ = svc.lock().map(|mut o| o.write_all(s.as_bytes()));
                    }
                }
            }
        }
        Kind::ApprovalReply { job_id, approval, yes } => {
            // only for a job that is running, that asked for this answer, and only if the person allowed answering from the web
            let ctl = sh.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(&job_id).cloned();
            if let Some(ctl) = ctl.filter(|c| c.approvals_from_web) {
                if let Some(tx) = ctl.waiting.lock().unwrap_or_else(|e| e.into_inner()).get(&approval) {
                    let _ = tx.send(yes);
                }
            }
        }
    }
}

/// Handles the jobs of one poll answer.
fn handle_jobs(sh: &Arc<Shared>, base: &str, cred: &str, answer: &Value) {
    let Some(list) = answer.get("jobs").and_then(Value::as_array) else { return };
    for raw in list.iter().take(8) {
        match parse_job(raw, now_ms()) {
            Ok(job) => start_job(sh, base, cred, job),
            Err((Some(id), why)) => {
                // tell the server, so the job does not hang there
                let (base, cred) = (base.to_string(), cred.to_string());
                std::thread::spawn(move || {
                    let mut b = vec![json!({"event": "error", "error": why})];
                    let _ = flush(&base, &cred, &id, &mut b, true);
                });
            }
            Err((None, _)) => {}
        }
    }
}

/// One round of the loop. Returns how many seconds to wait before the next, or 0 to go on at once.
fn poll_once(sh: &Arc<Shared>, ep: &Endpoint, name: &str, failures: &mut u32) -> u64 {
    let Some((base, cred)) = ep() else {
        st().connected = false;
        // the switch is off (or nobody is signed in): no terminal stays open for the web
        for ctl in sh.terms.lock().unwrap_or_else(|e| e.into_inner()).values() {
            ctl.stop.store(true, Ordering::SeqCst);
        }
        return 2;
    };
    let body = json!({"name": name, "caps": caps()});
    match post(&format!("{base}/api/link/poll"), &cred, &body, Duration::from_secs(35)) {
        Ok(v) => {
            *failures = 0;
            {
                let mut s = st();
                s.connected = true;
                s.last_ok_ms = now_ms();
                s.last_error.clear();
            }
            handle_jobs(sh, &base, &cred, &v);
            // a poll that came back empty is the server's 25 s wait running out: poll again at once
            0
        }
        Err(f) => {
            *failures += 1;
            let wait = if f.status == 429 { f.retry_after.clamp(2, 120) } else if f.status == 401 { 60 } else { backoff_secs(*failures) };
            let mut s = st();
            s.connected = false;
            s.last_error = if f.status == 401 { "The server does not accept this sign-in any more. Sign in again.".into() } else { f.message };
            wait
        }
    }
}

/// Runs the loop on the calling thread until `quit` is set. Nothing goes over the network while `ep` gives None.
pub fn run_loop(host: Arc<dyn Host>, ep: Endpoint, connect: Connect, quit: Arc<AtomicBool>) {
    let sh = Arc::new(Shared { host, connect, jobs: Mutex::new(HashMap::new()), terms: Mutex::new(HashMap::new()), finished: AtomicU64::new(0) });
    run_loop_with(sh, ep, quit);
}

fn run_loop_with(sh: Arc<Shared>, ep: Endpoint, quit: Arc<AtomicBool>) {
    let mut failures = 0u32;
    while !quit.load(Ordering::SeqCst) {
        let wait = poll_once(&sh, &ep, &device_name(), &mut failures);
        if wait > 0 {
            let until = Instant::now() + Duration::from_secs(wait);
            while Instant::now() < until && !quit.load(Ordering::SeqCst) {
                if WAKE.swap(false, Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    // the switch went off or the app quits: whatever a web session still runs is stopped
    for ctl in sh.jobs.lock().unwrap_or_else(|e| e.into_inner()).values().chain(sh.terms.lock().unwrap_or_else(|e| e.into_inner()).values()) {
        ctl.stop.store(true, Ordering::SeqCst);
    }
}

/// Starts the link thread (once). It does nothing, and touches no network, until the switch is on and someone is signed in.
pub fn start(host: Arc<dyn Host>) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(move || run_loop(host, default_endpoint(), default_connect(), Arc::new(AtomicBool::new(false))));
    });
}

/// Turns the link on or off from a screen. Turning it off also stops a web session that is running.
pub fn set_enabled(on: bool) -> Result<(), String> {
    update_config(|c| c.enabled = on)
}

// ---------------------------------------------------------------------------------------------------------------------------
// Claude Code token (owner only)

/// A secret that is overwritten with zeros when it is dropped. It has no Debug and no Display, so it cannot be logged by accident.
pub struct Secret(String);

impl Secret {
    pub fn new(s: String) -> Secret {
        Secret(s)
    }
    /// For sending only.
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn wipe(s: &mut String) {
    // SAFETY: zero bytes are valid UTF-8; volatile writes so the compiler does not remove them.
    unsafe {
        for b in s.as_bytes_mut() {
            std::ptr::write_volatile(b, 0);
        }
    }
    s.clear();
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

const TOKEN_PREFIX: &str = "sk-ant-oat";

/// Looks like the long-lived token `claude setup-token` prints (sk-ant-oat01-...): the prefix, then letters, digits, - and _.
pub fn valid_token(t: &str) -> bool {
    t.starts_with(TOKEN_PREFIX) && t.len() >= 40 && t.len() <= 600 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Finds the token in what a terminal shows (the last one, when there are several). Pasted text works too: it is trimmed.
pub fn extract_token(text: &str) -> Option<Secret> {
    let mut found = None;
    let mut rest = text;
    while let Some(i) = rest.find(TOKEN_PREFIX) {
        let tail = &rest[i..];
        let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(tail.len());
        let cand = &tail[..end];
        if valid_token(cand) {
            found = Some(cand.to_string());
        }
        rest = &tail[end.max(TOKEN_PREFIX.len())..];
    }
    found.map(Secret::new)
}

/// Sends the token to the person's own server account (admin accounts only: the server refuses everybody else with a message).
pub fn send_claude_token_at(base: &str, cred: &str, token: &Secret) -> Result<(), String> {
    if !valid_token(token.expose()) {
        return Err("That does not look like a Claude sign-in token (it starts with sk-ant-oat).".into());
    }
    let r = post(&format!("{base}/api/sync/claude_token"), cred, &json!({"token": token.expose()}), Duration::from_secs(30)).map_err(|f| friendly(f))?;
    if r.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(r.get("error").and_then(Value::as_str).unwrap_or("The server refused the token.").chars().take(300).collect());
    }
    Ok(())
}

/// Removes the token from the server.
pub fn remove_claude_token_at(base: &str, cred: &str) -> Result<(), String> {
    let r = post(&format!("{base}/api/sync/claude_token"), cred, &json!({"delete": true}), Duration::from_secs(30)).map_err(friendly)?;
    if r.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(r.get("error").and_then(Value::as_str).unwrap_or("The server refused.").chars().take(300).collect());
    }
    Ok(())
}

fn friendly(f: Failure) -> String {
    match f.status {
        401 => "The server does not accept this sign-in. Sign in again.".into(),
        403 => if f.message.starts_with("The server answered") { "Only the owner account of the server can do this.".into() } else { f.message },
        _ => f.message,
    }
}

fn server_and_credential() -> Result<(String, String), String> {
    let cred = crate::login::credential_of(crate::login::Client::Desktop).ok_or("Sign in first (Settings > Account and sync).")?;
    Ok((crate::login::server_url()?, cred))
}

/// Sends the token to the signed-in account's server; remembers only WHEN (not the token).
pub fn send_claude_token(token: &Secret) -> Result<(), String> {
    let (base, cred) = server_and_credential()?;
    send_claude_token_at(&base, &cred, token)?;
    let _ = update_config(|c| c.claude_sent_at = now_ms());
    Ok(())
}

pub fn remove_claude_token() -> Result<(), String> {
    let (base, cred) = server_and_credential()?;
    remove_claude_token_at(&base, &cred)?;
    let _ = update_config(|c| c.claude_sent_at = 0);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------------
// tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    // ---- pure parts

    #[test]
    fn the_cap_never_gives_bypass_unless_allowed() {
        assert_eq!(cap_mode(Some(Mode::Bypass), Mode::Auto, false), Mode::Auto);
        assert_eq!(cap_mode(None, Mode::Bypass, false), Mode::Auto, "a saved Bypass is capped too");
        assert_eq!(cap_mode(Some(Mode::Bypass), Mode::Auto, true), Mode::Bypass);
        assert_eq!(cap_mode(Some(Mode::Manual), Mode::Auto, false), Mode::Manual, "stricter stays stricter");
        assert_eq!(cap_mode(None, Mode::Manual, false), Mode::Manual);
        assert_eq!(cap_mode(Some(Mode::Auto), Mode::Manual, false), Mode::Manual, "the web cannot loosen the saved mode");
        assert_eq!(cap_mode(Some(Mode::Auto), Mode::Manual, true), Mode::Manual);
        assert_eq!(cap_mode(Some(Mode::Bypass), Mode::Manual, false), Mode::Manual);
    }

    #[test]
    fn jobs_are_validated() {
        let ok = parse_job(&json!({"id": "j1", "type": "send", "text": "hi", "chat": "c123", "mode": "manual"}), 0).unwrap();
        assert_eq!(ok.kind, Kind::Send { text: "hi".into(), chat: Some("c123".into()), edit: None, attachments: None, mode: Some(Mode::Manual) });
        let why = |v: Value| parse_job(&v, 1_000_000).err().map(|e| e.1).unwrap_or_default();
        assert!(why(json!({"type": "send", "text": "x"})).contains("usable id"));
        assert!(why(json!({"id": "a b", "type": "send", "text": "x"})).contains("usable id"));
        assert!(why(json!({"id": "j", "type": "shell", "cmd": "rm -rf /"})).contains("Unknown job type"));
        assert!(why(json!({"id": "j", "type": "send", "text": "   "})).contains("empty"));
        assert!(why(json!({"id": "j", "type": "send", "text": "x".repeat(MAX_TEXT_CHARS + 1)})).contains("too long"));
        assert!(why(json!({"id": "j", "type": "send", "text": "x", "chat": "../../etc"})).contains("chat id"));
        assert!(why(json!({"id": "j", "type": "send", "text": "x", "mode": "yolo"})).contains("mode"));
        assert!(why(json!({"id": "j", "type": "send", "text": "x", "attachments": "no"})).contains("attachments"));
        assert!(why(json!({"id": "j", "type": "op", "op": "delete_all_chats"})).contains("not allowed"));
        assert!(why(json!({"id": "j", "type": "op", "inner": {"op": "set_key", "key": "x"}})).contains("not allowed"));
        assert!(why(json!({"id": "j", "type": "op", "op": "select", "inner": {"provider": ["a"]}})).contains("argument"));
        assert!(why(json!({"id": "j", "type": "approval_reply", "job_id": "x"})).contains("approval"));
        // a job that waited too long: by its age, or by its creation time
        assert!(why(json!({"id": "j", "type": "send", "text": "x", "age_ms": MAX_JOB_AGE_MS})).contains("too long"));
        assert!(why(json!({"id": "j", "type": "send", "text": "x", "created_ms": 1_000_000 - MAX_JOB_AGE_MS})).contains("too long"));
        assert!(parse_job(&json!({"id": "j", "type": "send", "text": "x", "age_ms": MAX_JOB_AGE_MS - 1}), 0).is_ok());
        // an operation keeps only its known arguments
        let op = parse_job(&json!({"id": "j", "type": "op", "op": "select", "inner": {"provider": "ollama", "tenant": {"id": "x"}, "key": "k"}}), 0).unwrap();
        assert_eq!(op.kind, Kind::Op { op: "select".into(), args: vec![("provider".into(), json!("ollama"))] });
        let s = parse_job(&json!({"id": "j", "type": "stop", "job_id": "abc"}), 0).unwrap();
        assert_eq!(s.kind, Kind::Stop { job_id: Some("abc".into()) });
    }

    #[test]
    fn the_service_request_is_built_from_checked_fields_only() {
        let r = send_request("hi", &Some("c1".into()), None, &None, Mode::Auto);
        assert_eq!(r, json!({"op": "send", "text": "hi", "client": "web-link", "mode": "auto", "can_ask": true, "chat": "c1"}));
        // the web can only make the saved mode stricter
        let args = |s: &str| vec![("set".to_string(), json!(s))];
        assert!(op_request("mode", &args("bypass"), Mode::Auto).is_err());
        assert!(op_request("mode", &args("auto"), Mode::Manual).is_err());
        assert!(op_request("mode", &args("manual"), Mode::Auto).is_ok());
        assert!(op_request("mode", &args("???"), Mode::Auto).is_err());
        assert!(op_request("sync_status", &[], Mode::Auto).is_err());
        assert_eq!(op_request("status", &[], Mode::Auto).unwrap()["client"], "web-link");
    }

    #[test]
    fn the_token_is_found_in_terminal_text() {
        let tok = format!("sk-ant-oat01-{}", "aB3_-".repeat(20));
        let screen = format!("Opening browser...\nYour OAuth token (valid for 1 year):\n\n{tok}\n\nStore this token securely.\n");
        let got = extract_token(&screen).unwrap();
        assert_eq!(got.expose(), tok);
        // pasted with spaces around
        assert_eq!(extract_token(&format!("  {tok}  \n")).unwrap().expose(), tok);
        // not a token: too short, wrong prefix
        assert!(extract_token("sk-ant-oat01-short").is_none());
        assert!(extract_token("sk-ant-api03-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_none());
        assert!(extract_token("no token here").is_none());
        assert!(!valid_token(&format!("{tok} x")));
    }

    #[test]
    fn a_secret_is_wiped_and_never_printed() {
        let mut s = String::from("sk-ant-oat01-secret");
        let p = s.as_ptr();
        let n = s.len();
        wipe(&mut s);
        // the buffer is still ours (only cleared), so its bytes can be looked at
        let after = unsafe { std::slice::from_raw_parts(p, n) };
        assert!(after.iter().all(|b| *b == 0), "the secret text is still in memory");
        assert!(s.is_empty());
    }

    #[test]
    fn backoff_grows_and_stops_at_a_minute() {
        assert_eq!([1, 2, 3, 6, 9].map(backoff_secs), [2, 4, 8, 60, 60]);
    }

    // ---- a small HTTP server that plays the ZO-AI server

    type Handler = Arc<dyn Fn(&str, &Value, &str) -> (u16, Value) + Send + Sync>;

    /// Returns its base address. `h(path, body, authorization)` answers each request.
    fn mock_server(h: Handler) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in l.incoming().flatten() {
                let h = h.clone();
                std::thread::spawn(move || {
                    let mut conn = conn;
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 8192];
                    let (head_end, len) = loop {
                        let n = conn.read(&mut tmp).unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                            let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                            break (i + 4, len);
                        }
                    };
                    while buf.len() < head_end + len {
                        let n = conn.read(&mut tmp).unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                    let auth = head.lines().find(|l| l.to_lowercase().starts_with("authorization:")).map(|l| l[14..].trim().to_string()).unwrap_or_default();
                    let body: Value = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap_or_default();
                    let (code, v) = h(&path, &body, &auth);
                    let b = v.to_string();
                    let _ = write!(conn, "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}", b.len());
                });
            }
        });
        base
    }

    struct TestHost {
        asked: Mutex<Vec<String>>,
        answer: bool,
    }
    impl Host for TestHost {
        fn ask_local(&self, summary: &str) -> bool {
            self.asked.lock().unwrap().push(summary.to_string());
            self.answer
        }
    }

    /// A fake service that plays one `send`: replies with the given events, waiting for approvals and stop where asked.
    fn fake_service(script: impl Fn(&Value, &mut UnixStream, &mut BufReader<UnixStream>) + Send + Sync + 'static) -> Connect {
        let script = Arc::new(script);
        Arc::new(move || {
            let (client, server) = UnixStream::pair()?;
            let script = script.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(server.try_clone().unwrap());
                let mut w = server;
                let mut l = String::new();
                if r.read_line(&mut l).unwrap_or(0) == 0 {
                    return;
                }
                let req: Value = serde_json::from_str(&l).unwrap();
                script(&req, &mut w, &mut r);
            });
            Ok(client)
        })
    }

    fn wl(w: &mut UnixStream, v: Value) {
        let _ = writeln!(w, "{v}");
    }

    fn wait_until(what: &str, f: impl Fn() -> bool) {
        let t = Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(15), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    struct Rig {
        quit: Arc<AtomicBool>,
        polls: Arc<AtomicU64>,
        events: Arc<Mutex<Vec<Value>>>,
        done: Arc<Mutex<Vec<String>>>,
        shared: Arc<Shared>,
        /// What the mock server hands to a terminal in the answers to its event posts.
        term_in: Arc<Mutex<Vec<Value>>>,
    }
    impl Drop for Rig {
        fn drop(&mut self) {
            self.quit.store(true, Ordering::SeqCst);
        }
    }

    /// Starts the loop against a mock server that hands out `jobs` on the first poll (and an empty answer, slowly, after that).
    /// `cancel_after` makes the events answer `{cancel:true}` once that many events came.
    fn rig(jobs: Vec<Value>, connect: Connect, host: Arc<dyn Host>, cancel_after: Option<usize>) -> Rig {
        let polls = Arc::new(AtomicU64::new(0));
        let events = Arc::new(Mutex::new(Vec::<Value>::new()));
        let done = Arc::new(Mutex::new(Vec::<String>::new()));
        let jobs = Arc::new(Mutex::new(Some(jobs)));
        let term_in = Arc::new(Mutex::new(Vec::<Value>::new()));
        let (p2, e2, d2, t2) = (polls.clone(), events.clone(), done.clone(), term_in.clone());
        let base = mock_server(Arc::new(move |path, body, auth| {
            if auth != "Bearer zd_test_credential_000000" {
                return (401, json!({"ok": false, "error": "no"}));
            }
            match path {
                "/api/link/poll" => {
                    p2.fetch_add(1, Ordering::SeqCst);
                    let j = jobs.lock().unwrap().take().unwrap_or_default();
                    if j.is_empty() {
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    (200, json!({"ok": true, "jobs": j}))
                }
                "/api/link/events" => {
                    let mut e = e2.lock().unwrap();
                    for x in body["events"].as_array().cloned().unwrap_or_default() {
                        e.push(x);
                    }
                    if body["done"] == true {
                        d2.lock().unwrap().push(body["job_id"].as_str().unwrap_or("").to_string());
                    }
                    let cancel = cancel_after.map(|n| e.len() >= n).unwrap_or(false);
                    let term: Vec<Value> = t2.lock().unwrap().drain(..).collect();
                    (200, json!({"ok": true, "cancel": cancel, "term": term}))
                }
                _ => (404, json!({"ok": false})),
            }
        }));
        let ep: Endpoint = Arc::new(move || Some((base.clone(), "zd_test_credential_000000".to_string())));
        let sh = Arc::new(Shared { host, connect, jobs: Mutex::new(HashMap::new()), terms: Mutex::new(HashMap::new()), finished: AtomicU64::new(0) });
        let quit = Arc::new(AtomicBool::new(false));
        let (sh2, q2) = (sh.clone(), quit.clone());
        std::thread::spawn(move || run_loop_with(sh2, ep, q2));
        Rig { quit, polls, events, done, shared: sh, term_in }
    }

    /// A private config/data/runtime folder, and the lock that keeps tests with their own folders apart.
    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, PathBuf) {
        let g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = PathBuf::from(format!("/tmp/zl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("run")).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", dir.join("c"));
        std::env::set_var("XDG_DATA_HOME", dir.join("d"));
        std::env::set_var("XDG_RUNTIME_DIR", dir.join("run"));
        (g, dir)
    }

    fn host(answer: bool) -> Arc<TestHost> {
        Arc::new(TestHost { asked: Mutex::new(Vec::new()), answer })
    }

    #[test]
    fn a_send_job_is_run_and_its_events_come_back() {
        let (_g, _dir) = sandbox("a_send");
        let seen_req = Arc::new(Mutex::new(Value::Null));
        let s2 = seen_req.clone();
        let connect = fake_service(move |req, w, _| {
            *s2.lock().unwrap() = req.clone();
            wl(w, json!({"event": "chat", "id": "c9", "title": "t"}));
            wl(w, json!({"event": "delta", "text": "Hel"}));
            wl(w, json!({"event": "text", "text": "Hello"}));
            wl(w, json!({"event": "done"}));
        });
        let job = json!({"id": "j-1", "type": "send", "text": "hi", "mode": "bypass", "tenant": {"id": "evil"}, "set": "bypass"});
        let r = rig(vec![job], connect, host(true), None);
        wait_until("the job to be done", || r.done.lock().unwrap().contains(&"j-1".to_string()));
        let kinds: Vec<String> = r.events.lock().unwrap().iter().map(|e| e["event"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["chat", "delta", "text", "done"], "{kinds:?}");
        // the request the service saw: capped to Auto, no tenant, no other keys
        let req = seen_req.lock().unwrap().clone();
        assert_eq!(req, json!({"op": "send", "text": "hi", "client": "web-link", "mode": "auto", "can_ask": true}));
        assert!(r.polls.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn an_unknown_job_type_and_an_expired_job_are_refused_and_reported() {
        let (_g, _dir) = sandbox("an_unk");
        let ran = Arc::new(AtomicBool::new(false));
        let r2 = ran.clone();
        let connect = fake_service(move |_, _, _| r2.store(true, Ordering::SeqCst));
        let jobs = vec![
            json!({"id": "bad", "type": "shell", "cmd": "id"}),
            json!({"id": "old", "type": "send", "text": "hi", "age_ms": 130_000}),
            json!({"id": "op", "type": "op", "op": "delete_all_chats"}),
        ];
        let r = rig(jobs, connect, host(true), None);
        wait_until("all three refused", || r.done.lock().unwrap().len() == 3);
        assert!(!ran.load(Ordering::SeqCst), "nothing reached the service");
        let ev = r.events.lock().unwrap().clone();
        assert!(ev.iter().all(|e| e["event"] == "error"), "{ev:?}");
        assert!(ev.iter().any(|e| e["error"].as_str().unwrap().contains("waited too long")));
    }

    #[test]
    fn the_server_can_cancel_a_running_job() {
        let (_g, _dir) = sandbox("the_se");
        let stopped = Arc::new(AtomicBool::new(false));
        let s2 = stopped.clone();
        let connect = fake_service(move |_, w, r| {
            wl(w, json!({"event": "chat", "id": "c9", "title": "t"}));
            // streams until it hears stop
            let reader = r.get_mut().try_clone().unwrap();
            let s3 = s2.clone();
            std::thread::spawn(move || {
                for l in BufReader::new(reader).lines().map_while(Result::ok) {
                    if l.contains("\"stop\"") {
                        s3.store(true, Ordering::SeqCst);
                    }
                }
            });
            for _ in 0..400 {
                if s2.load(Ordering::SeqCst) {
                    wl(w, json!({"event": "stopped"}));
                    wl(w, json!({"event": "done"}));
                    return;
                }
                wl(w, json!({"event": "delta", "text": "la "}));
                std::thread::sleep(Duration::from_millis(25));
            }
            wl(w, json!({"event": "done"}));
        });
        let r = rig(vec![json!({"id": "j-c", "type": "send", "text": "go"})], connect, host(true), Some(3));
        wait_until("the job to end", || r.done.lock().unwrap().contains(&"j-c".to_string()));
        assert!(stopped.load(Ordering::SeqCst), "the service was told to stop");
        assert!(r.events.lock().unwrap().iter().any(|e| e["event"] == "stopped"));
    }

    #[test]
    fn an_approval_is_asked_on_this_computer_and_the_web_only_hears_that_it_waits() {
        let (_g, _dir) = sandbox("an_app");
        let answer = Arc::new(Mutex::new(None::<bool>));
        let a2 = answer.clone();
        let connect = fake_service(move |_, w, r| {
            wl(w, json!({"event": "approval", "id": 7, "tool": "run_command", "summary": "run: rm x"}));
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            let v: Value = serde_json::from_str(&l).unwrap();
            assert_eq!(v["op"], "approve");
            *a2.lock().unwrap() = v["yes"].as_bool();
            wl(w, json!({"event": "done"}));
        });
        let h = host(true);
        let r = rig(vec![json!({"id": "j-a", "type": "send", "text": "do it"})], connect, h.clone(), None);
        wait_until("done", || r.done.lock().unwrap().contains(&"j-a".to_string()));
        assert_eq!(*answer.lock().unwrap(), Some(true));
        assert_eq!(h.asked.lock().unwrap().as_slice(), ["run: rm x"]);
        let ev = r.events.lock().unwrap().clone();
        let ap = ev.iter().find(|e| e["event"] == "approval").unwrap();
        assert_eq!(ap["local"], true);
    }

    #[test]
    fn a_denial_on_this_computer_is_a_denial() {
        let (_g, _dir) = sandbox("a_deni");
        let answer = Arc::new(Mutex::new(None::<bool>));
        let a2 = answer.clone();
        let connect = fake_service(move |_, w, r| {
            wl(w, json!({"event": "approval", "id": 1, "tool": "t", "summary": "s"}));
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            *a2.lock().unwrap() = serde_json::from_str::<Value>(&l).unwrap()["yes"].as_bool();
            wl(w, json!({"event": "done"}));
        });
        let r = rig(vec![json!({"id": "j-d", "type": "send", "text": "x"})], connect, host(false), None);
        wait_until("done", || r.done.lock().unwrap().contains(&"j-d".to_string()));
        assert_eq!(*answer.lock().unwrap(), Some(false));
    }

    #[test]
    fn web_approvals_are_ignored_unless_the_person_allowed_them() {
        // approvals_from_web is read from link.json: here it is off (no file), so an approval_reply must change nothing
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-link-ap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        let got = Arc::new(Mutex::new(None::<bool>));
        let g2 = got.clone();
        let connect = fake_service(move |_, w, r| {
            wl(w, json!({"event": "approval", "id": 3, "tool": "t", "summary": "s"}));
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            *g2.lock().unwrap() = serde_json::from_str::<Value>(&l).unwrap()["yes"].as_bool();
            wl(w, json!({"event": "done"}));
        });
        // the person denies locally after a moment; a forged "yes" from the web arrives first
        struct Slow;
        impl Host for Slow {
            fn ask_local(&self, _: &str) -> bool {
                std::thread::sleep(Duration::from_millis(600));
                false
            }
        }
        let r = rig(vec![json!({"id": "j-w", "type": "send", "text": "x"})], connect, Arc::new(Slow), None);
        wait_until("the job to wait", || r.shared.jobs.lock().unwrap().contains_key("j-w"));
        let ctl = r.shared.jobs.lock().unwrap().get("j-w").cloned().unwrap();
        assert!(!ctl.approvals_from_web);
        start_job(&r.shared, "http://127.0.0.1:9", "x", Job { id: "r".into(), kind: Kind::ApprovalReply { job_id: "j-w".into(), approval: 3, yes: true } });
        wait_until("done", || r.done.lock().unwrap().contains(&"j-w".to_string()));
        assert_eq!(*got.lock().unwrap(), Some(false), "the forged yes did not count");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn web_approvals_work_when_allowed() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-link-aw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        save_config(&Config { approvals_from_web: true, ..Default::default() }).unwrap();
        let got = Arc::new(Mutex::new(None::<bool>));
        let g2 = got.clone();
        let connect = fake_service(move |_, w, r| {
            wl(w, json!({"event": "approval", "id": 3, "tool": "t", "summary": "s"}));
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            *g2.lock().unwrap() = serde_json::from_str::<Value>(&l).unwrap()["yes"].as_bool();
            wl(w, json!({"event": "done"}));
        });
        let h = host(false);
        let r = rig(vec![json!({"id": "j-v", "type": "send", "text": "x"})], connect, h.clone(), None);
        wait_until("the approval to be relayed", || r.events.lock().unwrap().iter().any(|e| e["event"] == "approval"));
        assert_eq!(r.events.lock().unwrap().iter().find(|e| e["event"] == "approval").unwrap()["local"], false);
        // an answer for a job or approval that does not exist does nothing
        start_job(&r.shared, "http://127.0.0.1:9", "x", Job { id: "r0".into(), kind: Kind::ApprovalReply { job_id: "other".into(), approval: 3, yes: true } });
        start_job(&r.shared, "http://127.0.0.1:9", "x", Job { id: "r1".into(), kind: Kind::ApprovalReply { job_id: "j-v".into(), approval: 3, yes: true } });
        wait_until("done", || r.done.lock().unwrap().contains(&"j-v".to_string()));
        assert_eq!(*got.lock().unwrap(), Some(true));
        assert!(h.asked.lock().unwrap().is_empty(), "nothing was asked locally");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A real home folder with a few files for the control operations (HOME is process-global: callers hold the shared test lock).
    fn with_home<T>(dir: &std::path::Path, f: impl FnOnce(&std::path::Path) -> T) -> T {
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        std::fs::write(home.join("docs/a.txt"), "hello").unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/id_ed25519"), "PRIVATE").unwrap();
        let old = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        let r = f(&std::fs::canonicalize(&home).unwrap());
        match old {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        r
    }

    #[test]
    fn control_jobs_are_checked_like_every_other_job() {
        let ok = |v: Value| parse_job(&v, 0).unwrap();
        let args = |v: Value| match ok(v).kind {
            Kind::Op { op, args } => (op, args),
            k => panic!("{k:?}"),
        };
        assert_eq!(args(json!({"id": "c1", "type": "op", "inner": {"op": "fs_list", "path": "~/docs"}})), ("fs_list".into(), vec![("path".into(), json!("~/docs"))]));
        // nothing but the path gets through to a control operation, and the settings operations never get a path
        assert_eq!(args(json!({"id": "c2", "type": "op", "inner": {"op": "fs_read", "path": "x", "tenant": "t", "mode": "bypass", "provider": "p"}})).1, vec![("path".to_string(), json!("x"))]);
        assert_eq!(args(json!({"id": "c3", "type": "op", "inner": {"op": "models", "provider": "p", "path": "/etc"}})).1, vec![("provider".to_string(), json!("p"))]);
        assert_eq!(args(json!({"id": "c4", "type": "op", "inner": {"op": "screenshot"}})), ("screenshot".into(), vec![]));
        let why = |v: Value| parse_job(&v, 0).unwrap_err().1;
        assert!(why(json!({"id": "c5", "type": "op", "inner": {"op": "fs_list", "path": ["a"]}})).contains("path"));
        assert!(why(json!({"id": "c6", "type": "op", "inner": {"op": "fs_list", "path": "a\nb"}})).contains("path"));
        assert!(why(json!({"id": "c7", "type": "op", "inner": {"op": "fs_list", "path": "x".repeat(2000)}})).contains("path"));
        // the ones phase 1 does not have are still refused
        for op in ["fs_write", "fs_rm", "fs_mkdir", "fs_move", "term_open", "term_input", "app_launch", "power", "terminal_run"] {
            assert!(why(json!({"id": "c8", "type": "op", "inner": {"op": op, "path": "x"}})).contains("not allowed"), "{op}");
        }
    }

    #[test]
    fn a_folder_is_listed_only_after_a_yes_on_this_computer() {
        let (_g, dir) = sandbox("ctl_list");
        with_home(&dir, |home| {
            // yes
            let h = host(true);
            let r = control_reply(&*h, "fs_list", &[("path".into(), json!("~/docs"))], Instant::now()).unwrap();
            assert_eq!(r["entries"][0]["name"], "a.txt");
            assert_eq!(h.asked.lock().unwrap().len(), 1);
            assert!(h.asked.lock().unwrap()[0].contains("~/docs"), "the dialog names the resolved folder: {:?}", h.asked.lock().unwrap());
            // no: nothing is read, and the answer says so
            let h = host(false);
            let e = control_reply(&*h, "fs_read", &[("path".into(), json!("docs/a.txt"))], Instant::now()).unwrap_err();
            assert!(e.contains("no on your computer"), "{e}");
            assert_eq!(h.asked.lock().unwrap().len(), 1);
            // a place of secrets or outside home: refused before anyone is asked
            let h = host(true);
            for bad in ["~/.ssh", ".ssh/id_ed25519", "/etc/passwd", "..", "~/docs/../.ssh"] {
                assert!(control_reply(&*h, "fs_read", &[("path".into(), json!(bad))], Instant::now()).is_err(), "{bad}");
                assert!(control_reply(&*h, "fs_list", &[("path".into(), json!(bad))], Instant::now()).is_err(), "{bad}");
            }
            assert!(h.asked.lock().unwrap().is_empty(), "refused paths never show a dialog: {:?}", h.asked.lock().unwrap());
            // the person's yes that comes after the web page gave up is not acted on
            let h = host(true);
            let e = control_reply(&*h, "fs_list", &[("path".into(), json!("~"))], Instant::now() - CONTROL_WAIT - Duration::from_secs(1)).unwrap_err();
            assert!(e.contains("too late"), "{e}");
            // reading a file gives its text
            let h = host(true);
            let r = control_reply(&*h, "fs_read", &[("path".into(), json!(home.join("docs/a.txt").to_string_lossy()))], Instant::now()).unwrap();
            assert_eq!(r["text"], "hello");
            // the audit log has every request, answered or not
            let log = std::fs::read_to_string(crate::audit::data_dir().join("audit.jsonl")).unwrap_or_default();
            for want in ["web:fs_list", "web:fs_read", "denied on the computer", "refused", "too late", "done"] {
                assert!(log.contains(want), "audit log lacks {want}: {log}");
            }
            assert!(!log.contains("PRIVATE"), "file contents are never logged");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn system_status_runs_without_asking_and_only_one_control_request_waits_at_a_time() {
        let (_g, dir) = sandbox("ctl_sys");
        let h = host(false);
        let r = control_reply(&*h, "sys_info", &[], Instant::now()).unwrap();
        assert_eq!(r["ok"], true);
        assert!(r["text"].as_str().unwrap().contains("CPU"));
        assert!(h.asked.lock().unwrap().is_empty(), "system status shows no personal content: no dialog");
        // a second request while one is busy is refused
        CONTROL_BUSY.store(true, Ordering::SeqCst);
        let e = control_reply(&*h, "sys_info", &[], Instant::now()).unwrap_err();
        CONTROL_BUSY.store(false, Ordering::SeqCst);
        assert!(e.contains("already waiting"), "{e}");
        assert!(!CONTROL_BUSY.load(Ordering::SeqCst));
        // and the flag is free again after a request that failed
        assert!(control_reply(&*h, "bogus", &[], Instant::now()).is_err());
        assert!(!CONTROL_BUSY.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_control_op_job_is_answered_through_the_link() {
        let (_g, dir) = sandbox("ctl_job");
        with_home(&dir, |_| {
            let h = host(true);
            let r = rig(vec![json!({"id": "f1", "type": "op", "inner": {"op": "fs_list", "path": "docs"}})], fake_service(|_, _, _| panic!("the service is not involved")), h.clone(), None);
            wait_until("done", || r.done.lock().unwrap().contains(&"f1".to_string()));
            let ev = r.events.lock().unwrap().clone();
            assert_eq!(ev[0]["event"], "op_result", "{ev:?}");
            assert_eq!(ev[0]["reply"]["entries"][0]["name"], "a.txt");
            assert_eq!(h.asked.lock().unwrap().len(), 1);
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_answer_job_is_checked_and_rebuilt_from_known_fields() {
        let ok = parse_job(&json!({"id": "a1", "type": "answer_reply", "job_id": "j-1", "question_id": 4, "answers": [{"selected": ["x"], "other": "t", "evil": 1}], "tenant": {"id": "z"}}), 0).unwrap();
        assert_eq!(ok.kind, Kind::AnswerReply { job_id: "j-1".into(), question: 4, reply: json!({"answers": [{"selected": ["x"], "other": "t"}]}) });
        let c = parse_job(&json!({"id": "a2", "type": "answer_reply", "job_id": "j-1", "question_id": "7", "cancelled": true}), 0).unwrap();
        assert_eq!(c.kind, Kind::AnswerReply { job_id: "j-1".into(), question: 7, reply: json!({"cancelled": true}) });
        let bad = |v: Value| parse_job(&v, 0).unwrap_err().1;
        let base = |extra: Value| { let mut b = json!({"id": "a3", "type": "answer_reply", "job_id": "j-1", "question_id": 1}); for (k, v) in extra.as_object().unwrap() { b[k] = v.clone(); } b };
        for extra in [json!({}), json!({"answers": []}), json!({"answers": "x"}), json!({"answers": [{"selected": [1]}]}), json!({"answers": [{"selected": ["a","b","c","d","e","f","g"]}]}),
                      json!({"answers": [{"selected": ["x".repeat(101)]}]}), json!({"answers": [{"other": "y".repeat(4001)}]}), json!({"answers": [{"other": 5}]}),
                      json!({"answers": [{}, {}, {}, {}, {}]}), json!({"job_id": "../x", "answers": [{"selected": ["a"]}]}), json!({"question_id": "abc", "answers": [{"selected": ["a"]}]})] {
            assert!(bad(base(extra.clone())).contains("not valid"), "{extra}");
        }
    }

    #[test]
    fn a_question_card_is_shown_to_the_web_and_only_its_answer_reaches_the_service() {
        let (_g, _dir) = sandbox("a_ques");
        let got = Arc::new(Mutex::new(Vec::<Value>::new()));
        let g2 = got.clone();
        let connect = fake_service(move |req, w, r| {
            assert_eq!(req["can_ask"], true, "the web page can show question cards");
            wl(w, json!({"event": "question", "id": 5, "questions": [{"question": "Which?", "header": "H", "multi_select": false, "options": [{"label": "a", "description": ""}, {"label": "b", "description": ""}]}]}));
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            g2.lock().unwrap().push(serde_json::from_str(&l).unwrap());
            wl(w, json!({"event": "done"}));
        });
        let r = rig(vec![json!({"id": "j-q", "type": "send", "text": "go"})], connect, host(true), None);
        wait_until("the question to reach the web", || r.events.lock().unwrap().iter().any(|e| e["event"] == "question"));
        assert_eq!(r.events.lock().unwrap().iter().find(|e| e["event"] == "question").unwrap()["questions"][0]["question"], "Which?");
        let reply = |job: &str, q: u64| start_job(&r.shared, "http://127.0.0.1:9", "x", Job { id: "r".into(), kind: Kind::AnswerReply { job_id: job.into(), question: q, reply: json!({"answers": [{"selected": ["b"], "other": null}]}) } });
        reply("other-job", 5); // not a running job
        reply("j-q", 99); // a card that was never shown
        std::thread::sleep(Duration::from_millis(200));
        assert!(got.lock().unwrap().is_empty(), "nothing reached the service for a wrong job or a wrong card");
        reply("j-q", 5);
        wait_until("done", || r.done.lock().unwrap().contains(&"j-q".to_string()));
        assert_eq!(got.lock().unwrap()[0], json!({"op": "answer", "id": 5, "answers": [{"selected": ["b"], "other": null}]}));
    }

    #[test]
    fn an_op_job_gets_the_services_answer() {
        let (_g, _dir) = sandbox("an_op_");
        let connect = fake_service(|req, w, _| {
            assert_eq!(req["op"], "profiles");
            wl(w, json!({"ok": true, "active": null, "profiles": []}));
        });
        let r = rig(vec![json!({"id": "o1", "type": "op", "inner": {"op": "profiles"}})], connect, host(true), None);
        wait_until("done", || r.done.lock().unwrap().contains(&"o1".to_string()));
        let ev = r.events.lock().unwrap().clone();
        assert_eq!(ev[0]["event"], "op_result");
        assert_eq!(ev[0]["reply"]["ok"], true);
    }

    #[test]
    fn nothing_goes_over_the_network_while_the_link_is_off_and_it_backs_off_when_the_server_is_down() {
        // off: the endpoint gives None, so no poll is made
        let polls = Arc::new(AtomicU64::new(0));
        let p2 = polls.clone();
        let base = mock_server(Arc::new(move |_, _, _| {
            p2.fetch_add(1, Ordering::SeqCst);
            (200, json!({"jobs": []}))
        }));
        let on = Arc::new(AtomicBool::new(false));
        let o2 = on.clone();
        let ep: Endpoint = Arc::new(move || o2.load(Ordering::SeqCst).then(|| (base.clone(), "zd_x".to_string())));
        let sh = Arc::new(Shared { host: host(true), connect: fake_service(|_, _, _| {}), jobs: Mutex::new(HashMap::new()), terms: Mutex::new(HashMap::new()), finished: AtomicU64::new(0) });
        let mut f = 0;
        assert_eq!(poll_once(&sh, &ep, "n", &mut f), 2);
        assert_eq!(polls.load(Ordering::SeqCst), 0, "no request while off");
        on.store(true, Ordering::SeqCst);
        assert_eq!(poll_once(&sh, &ep, "n", &mut f), 0);
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        // a server that is not there: failures give growing waits and an error text, and the app goes on
        let dead: Endpoint = Arc::new(|| Some(("http://127.0.0.1:9".to_string(), "zd_x".to_string())));
        let mut f = 0;
        assert_eq!(poll_once(&sh, &dead, "n", &mut f), 2);
        assert_eq!(poll_once(&sh, &dead, "n", &mut f), 4);
        assert_eq!(poll_once(&sh, &dead, "n", &mut f), 8);
        assert!(state().last_error.contains("Cannot reach"));
    }

    #[test]
    fn the_claude_token_goes_to_the_server_and_a_refusal_is_clear() {
        let tok = format!("sk-ant-oat01-{}", "xY1_-".repeat(20));
        let seen = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let s2 = seen.clone();
        let admin = Arc::new(AtomicBool::new(true));
        let a2 = admin.clone();
        let base = mock_server(Arc::new(move |path, body, auth| {
            s2.lock().unwrap().push((path.to_string(), body.clone()));
            assert_eq!(auth, "Bearer zd_cred");
            if a2.load(Ordering::SeqCst) {
                (200, json!({"ok": true}))
            } else {
                (403, json!({"ok": false, "error": "Only the owner of this server can use Claude Code here."}))
            }
        }));
        send_claude_token_at(&base, "zd_cred", &Secret::new(tok.clone())).unwrap();
        remove_claude_token_at(&base, "zd_cred").unwrap();
        {
            let s = seen.lock().unwrap();
            assert_eq!(s[0], ("/api/sync/claude_token".to_string(), json!({"token": tok})));
            assert_eq!(s[1].1, json!({"delete": true}));
        }
        admin.store(false, Ordering::SeqCst);
        let e = send_claude_token_at(&base, "zd_cred", &Secret::new(tok.clone())).unwrap_err();
        assert!(e.contains("owner"), "{e}");
        assert!(!e.contains("sk-ant"), "the token is never in an error");
        // a wrong token never leaves this computer
        let n = seen.lock().unwrap().len();
        assert!(send_claude_token_at(&base, "zd_cred", &Secret::new("hello".into())).is_err());
        assert_eq!(seen.lock().unwrap().len(), n);
    }

    #[test]
    fn the_settings_default_to_off_and_survive_a_round_trip() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-link-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        assert_eq!(config(), Config::default());
        assert!(!config().enabled && !config().approvals_from_web && !config().allow_bypass);
        set_enabled(true).unwrap();
        update_config(|c| c.name = "My laptop".into()).unwrap();
        let c = config();
        assert!(c.enabled && c.name == "My laptop" && !c.allow_bypass);
        update_config(|c| c.name = "x".repeat(200)).unwrap();
        assert_eq!(config().name.len(), 60, "a long name is cut");
        assert!(update_config(|c| c.name = "\u{1}".into()).is_err());
        let text = std::fs::read_to_string(dir.join("zo-ai/link.json")).unwrap();
        assert!(!text.contains("sk-ant"), "no token in the settings file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- the real service

    /// A model server that answers one request with a fixed text (OpenAI style).
    fn model(answer: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let body = json!({"choices": [{"message": {"content": answer}}]}).to_string();
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let mut s = conn;
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = s.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        while buf.len() < i + 4 + len {
                            let n = s.read(&mut tmp).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&tmp[..n]);
                        }
                        break;
                    }
                }
                let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            }
        });
        url
    }

    #[test]
    fn a_send_job_runs_through_the_real_service_and_the_chat_is_saved() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = PathBuf::from(format!("/tmp/zl{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("run")).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", dir.join("c"));
        std::env::set_var("XDG_DATA_HOME", dir.join("d"));
        std::env::set_var("XDG_RUNTIME_DIR", dir.join("run"));
        let mut sel = crate::ai::Selection { provider: "ollama".into(), model: "m".into(), ..Default::default() };
        sel.base_urls.insert("ollama".into(), model("Hello from the laptop."));
        sel.save();
        crate::service::start().unwrap();
        let r = rig(vec![json!({"id": "real-1", "type": "send", "text": "hi from the web", "mode": "bypass"})], default_connect(), host(true), None);
        wait_until("the job to be done", || r.done.lock().unwrap().contains(&"real-1".to_string()));
        let ev = r.events.lock().unwrap().clone();
        assert!(ev.iter().any(|e| e["event"] == "text" && e["text"] == "Hello from the laptop."), "{ev:?}");
        assert!(ev.iter().any(|e| e["event"] == "chat"), "{ev:?}");
        let saved = crate::store::chats::load();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].title, "hi from the web");
        // while it ran, it was listed as a web session
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn b64_decode(s: &str) -> Vec<u8> {
        let val = |c: u8| match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            _ => 63,
        } as u32;
        let b: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
        let mut out = Vec::new();
        for c in b.chunks(4) {
            let n = c.iter().enumerate().fold(0u32, |a, (i, x)| a | val(*x) << (18 - 6 * i));
            out.push((n >> 16) as u8);
            if c.len() > 2 {
                out.push((n >> 8) as u8);
            }
            if c.len() > 3 {
                out.push(n as u8);
            }
        }
        out
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..40usize {
            let data: Vec<u8> = (0..n as u8).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(b64_decode(&b64_encode(&data)), data, "{n}");
        }
        assert_eq!(b64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn a_terminal_job_is_validated() {
        let ok = parse_job(&json!({"id": "t1", "type": "term", "rows": 20, "cols": 80, "cmd": "rm -rf /", "tenant": {"id": "x"}}), 0).unwrap();
        assert_eq!(ok.kind, Kind::Term { rows: 20, cols: 80 }, "unknown fields are ignored: a job cannot carry a command");
        assert_eq!(parse_job(&json!({"id": "t2", "type": "term"}), 0).unwrap().kind, Kind::Term { rows: 30, cols: 110 });
        for bad in [json!({"rows": 0}), json!({"rows": 100000}), json!({"cols": "wide"}), json!({"cols": 3})] {
            let mut j = json!({"id": "t3", "type": "term"});
            for (k, v) in bad.as_object().unwrap() {
                j[k] = v.clone();
            }
            assert!(parse_job(&j, 0).is_err(), "{j}");
        }
    }

    #[test]
    fn a_web_terminal_is_not_opened_when_the_person_here_says_no() {
        let (_g, _dir) = sandbox("term_no");
        let h = host(false);
        let r = rig(vec![json!({"id": "t-no", "type": "term"})], fake_service(|_, _, _| {}), h.clone(), None);
        wait_until("the job to end", || r.done.lock().unwrap().contains(&"t-no".to_string()));
        let kinds: Vec<String> = r.events.lock().unwrap().iter().map(|e| e["event"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["waiting", "denied"], "{kinds:?}");
        assert_eq!(h.asked.lock().unwrap().as_slice(), [TERM_QUESTION.to_string()], "it was asked on this computer");
        assert!(r.shared.terms.lock().unwrap().is_empty());
    }

    #[test]
    fn a_web_terminal_runs_a_shell_for_the_page_and_ends_on_close() {
        let (_g, _dir) = sandbox("term_yes");
        let old = std::env::var_os("SHELL");
        std::env::set_var("SHELL", "/bin/sh");
        let h = host(true);
        let r = rig(vec![json!({"id": "t-yes", "type": "term", "rows": 24, "cols": 80})], fake_service(|_, _, _| {}), h.clone(), None);
        wait_until("the terminal to open", || r.events.lock().unwrap().iter().any(|e| e["event"] == "opened"));
        r.term_in.lock().unwrap().push(json!({"t": "in", "data": "echo zo-link-$((6*7)); stty size\n"}));
        let seen = |needle: &str| {
            let text: Vec<u8> = r.events.lock().unwrap().iter().filter(|e| e["event"] == "out").flat_map(|e| b64_decode(e["b"].as_str().unwrap())).collect();
            String::from_utf8_lossy(&text).contains(needle)
        };
        wait_until("the output of the typed command", || seen("zo-link-42"));
        assert!(seen("24 80"), "the size that was asked for");
        r.term_in.lock().unwrap().push(json!({"t": "resize", "rows": 40, "cols": 100}));
        std::thread::sleep(Duration::from_millis(400));
        r.term_in.lock().unwrap().push(json!({"t": "in", "data": "stty size\n"}));
        wait_until("the new size", || seen("40 100"));
        // anything that is not an input, a resize or a close is ignored
        r.term_in.lock().unwrap().push(json!({"t": "run", "cmd": "touch /tmp/zo-should-not-exist"}));
        r.term_in.lock().unwrap().push(json!({"t": "close"}));
        wait_until("the terminal to end", || r.done.lock().unwrap().contains(&"t-yes".to_string()));
        assert!(!std::path::Path::new("/tmp/zo-should-not-exist").exists());
        assert_eq!(h.asked.lock().unwrap().len(), 1);
        wait_until("the terminal to be forgotten", || r.shared.terms.lock().unwrap().is_empty());
        match old {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }
}
