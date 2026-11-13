//! Zohara Agent in the cloud, as a public service: anybody with a Google account may sign in and use the same brain as the desktop app,
//! each person in their own isolated corner (a "tenant") with their own API key, their data kept in Firestore because the server's
//! own disk does not last. Only the owner's addresses are admins; everybody else is restricted to a small set of safe tools.
//!
//! Settings (environment):
//!   PORT                       where to listen (Render sets it)
//!   FIREBASE_SERVICE_ACCOUNT   the service account key file's contents (JSON): turns on the login check and Firestore (required)
//!                              (this, ZO_SERVER_SECRET and CLAUDE_CODE_OAUTH_TOKEN are read at start and removed from the environment)
//!   ZO_ADMIN_EMAILS            the owner's address(es), separated by commas: admins (unrestricted tools, Claude Code, SSH, Bypass)
//!   ZO_BLOCKED_EMAILS          addresses that are refused (optional)
//!   ZO_TERMINAL=off            turns the cloud terminal (a shell for admins, see term.rs) off; ZO_MAX_TERMINALS (8), ZO_TERM_MEM_MB (2048), ZO_TERM_NPROC
//!   ZO_CLAUDE_CODE_USERS       admins (default) or all: who may sign in to Claude Code on the server (see docs/TERMINAL-AND-CLAUDE-LOGIN.md);
//!                              ZO_MAX_CLAUDE_LOGINS (2) sign-ins at the same time
//!   ZO_MAX_USERS               how many people may register (default 200)
//!   ZO_MAX_RUNS                answers being written at once on the whole server (default 8)
//!   ZO_CONTACT_EMAIL           the operator's address, shown in the privacy notice and the terms (required)
//!   ZO_HOSTING_REGION          where the server runs, for the privacy notice, for example "Frankfurt, Germany" (required)
//!   ZO_OPERATOR_COUNTRY        whose law the terms name (required)
//!   ZO_SERVER_SECRET           a long random text: each person's API keys are encrypted with a key derived from it
//!   ZO_TENANT_ROOT             where the people's folders live on the disk (default /tmp/zo-tenants; emptied at start, Firestore is the record)
//!   ZO_PUBLIC_URL              the server's public address, https://host with no path (for example https://zo-ai-server.onrender.com):
//!                              devices show people `<it>/device` to sign in. Else RENDER_EXTERNAL_URL is used. One of the two is
//!                              required (the server does not start with the login on and neither set, or one that is not https://host)
//!   RENDER_EXTERNAL_URL        the server's own address (Render sets it): it calls itself every few minutes so it is not put to sleep
//!   ZO_SERVER_NO_AUTH=1        for testing on your own computer only: no login, listens on 127.0.0.1 only, no Firestore, one fixed
//!                              (restricted) person; ZO_NOAUTH_ADMIN=1 makes that person an admin

mod account;
mod auth;
mod claude_api;
mod db;
mod devicelogin;
mod devices;
mod firestore;
mod hardening;
mod link;
mod loginapi;
mod pages;
mod push;
mod pwa;
mod sync;
mod sync_api;
mod tenants;
mod term;
#[cfg(test)]
mod testenv;

use auth::{Policy, Verifier};
use devices::{Devices, Miss, Scope};
use devicelogin::DeviceLogin;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tenants::{Lease, Refusal, TenantMap};
use tiny_http::{Header, Method, Request, Response, Server};

/// Request bodies: /api/op, /api/send and /api/reply, and the desktop sync.
const BODY_LIMIT: usize = 1_000_000;
const SYNC_BODY_LIMIT: usize = 20_000_000;
/// A message with attachments (a few shrunken pictures and the text of files) is the only other body that is allowed to be big.
const SEND_BODY_LIMIT: usize = 14_000_000;
/// Message bodies bigger than a plain one that are being read at the same time, for the whole server (each is held in memory).
const MAX_BIG_SENDS: usize = 16;
static BIG_SENDS: AtomicUsize = AtomicUsize::new(0);
/// How long an answer of somebody who left (`keep_running`) may still take: the answer limit of a restricted account is 120 s.
const KEEP_RESTRICTED_SECS: u64 = 130;
const KEEP_ADMIN_SECS: u64 = 1800;
/// Requests being handled at the same time, for the whole server.
const MAX_CONNECTIONS: usize = 400;

/// What a request is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Op,
    Send,
    Reply,
    Sync,
    Account,
    /// /api/auth/device_start and device_poll: nobody is signed in yet.
    DeviceStart,
    DevicePoll,
    /// /api/auth/device_lookup, device_approve and exchange: a person signed in with Firebase.
    Auth,
    /// /api/auth/signout: a device (or a person) signing out.
    Signout,
    /// /api/link/poll and /api/link/events: the desktop app's side of the desktop link (a desktop credential only).
    LinkDevice,
    /// /api/link/status: the linked computers (a person signed in; a desktop may ask too).
    LinkStatus,
    /// /api/sync/claude_token: the owner's desktop sends (or removes) the Claude sign-in (a desktop credential only).
    ClaudeToken,
    /// /api/term/*: terminals (see term.rs). A person signed in, or a `full` device (the `zo` terminal client); never a desktop's own credential.
    Term,
    /// /api/claude/*: the guided Claude Code sign-in on the server (see claude_api.rs).
    Claude,
}

/// How a caller proved who they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cred {
    /// A Firebase sign-in (or the local test person).
    Account { uid: String, email: String },
    /// A device credential; `id` is its entry in the person's list of signed-in devices.
    Device { scope: Scope, id: String },
}

#[derive(Clone, Debug)]
pub struct Who {
    pub cred: Cred,
    pub tenant: String,
    pub admin: bool,
}

/// Which routes a credential may call. A signed-in person: all but the desktop's own. A `full` device: the API, the sync and signing out.
/// A `sync` device (the desktop app): the sync, signing out, the desktop link and the Claude token. Approving a device and the account page are for a person signed in with Firebase only.
pub fn permits(cred: &Cred, route: Route) -> bool {
    match cred {
        // a web session is never a linked computer and never hands over a secret: the poll, the posting of events and the Claude
        // token are for the desktop app's own credential
        Cred::Account { .. } => !matches!(route, Route::LinkDevice | Route::ClaudeToken),
        Cred::Device { scope: Scope::Full, .. } => !matches!(route, Route::Account | Route::Auth | Route::LinkDevice | Route::ClaudeToken),
        Cred::Device { scope: Scope::Sync, .. } => matches!(route, Route::Sync | Route::Signout | Route::LinkDevice | Route::LinkStatus | Route::ClaudeToken),
    }
}

pub fn route_of(method: &Method, path: &str) -> Option<Route> {
    if *method != Method::Post {
        return None;
    }
    match path {
        "/api/op" => Some(Route::Op),
        "/api/send" => Some(Route::Send),
        "/api/reply" => Some(Route::Reply),
        "/api/sync/manifest" | "/api/sync/get" | "/api/sync/put" | "/api/sync/delete" => Some(Route::Sync),
        "/api/auth/device_start" => Some(Route::DeviceStart),
        "/api/auth/device_poll" => Some(Route::DevicePoll),
        "/api/auth/signout" => Some(Route::Signout),
        "/api/link/poll" | "/api/link/events" => Some(Route::LinkDevice),
        "/api/link/status" => Some(Route::LinkStatus),
        "/api/sync/claude_token" => Some(Route::ClaudeToken),
        p if p.strip_prefix("/api/term/").is_some_and(|n| term::CALLS.contains(&n)) => Some(Route::Term),
        p if p.strip_prefix("/api/claude/").is_some_and(|n| claude_api::CALLS.contains(&n)) => Some(Route::Claude),
        p if p.strip_prefix("/api/auth/").is_some_and(|n| loginapi::PERSON_CALLS.contains(&n)) => Some(Route::Auth),
        p => p.strip_prefix("/api/account/").filter(|n| account::CALLS.contains(n)).map(|_| Route::Account),
    }
}

/// A place among the big message bodies being read (see `MAX_BIG_SENDS`); given back when dropped.
struct BigSend<'a>(&'a AtomicUsize);

impl<'a> BigSend<'a> {
    fn take(n: &'a AtomicUsize) -> Option<BigSend<'a>> {
        if n.fetch_add(1, Ordering::SeqCst) >= MAX_BIG_SENDS {
            n.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(BigSend(n))
    }
}

impl Drop for BigSend<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How many big message bodies one person may have being read at once: one account cannot take all the places of `MAX_BIG_SENDS`.
const MAX_BIG_SENDS_PER_PERSON: usize = 2;
static BIG_SENDS_BY_PERSON: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// One person's place among the big message bodies; given back when dropped.
struct BigShare(String);

impl BigShare {
    fn take(who: &str) -> Option<BigShare> {
        let mut all = BIG_SENDS_BY_PERSON.lock().unwrap_or_else(|e| e.into_inner());
        if all.iter().filter(|w| *w == who).count() >= MAX_BIG_SENDS_PER_PERSON {
            return None;
        }
        all.push(who.to_string());
        Some(BigShare(who.to_string()))
    }
}

impl Drop for BigShare {
    fn drop(&mut self) {
        let mut all = BIG_SENDS_BY_PERSON.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = all.iter().position(|w| *w == self.0) {
            all.remove(i);
        }
    }
}

pub struct App {
    pub no_auth: bool,
    pub noauth_admin: bool,
    pub verifier: Option<Arc<dyn Verifier>>,
    pub policy: Policy,
    pub tenants: Arc<TenantMap>,
    pub devices: Option<Devices>,
    /// The waiting requests of the browser device login (`devicelogin.rs`).
    pub login: DeviceLogin,
    /// `https://host`, validated: the approval page is `<it>/device`.
    pub public_url: Option<String>,
    /// Answers running right now, with the person they belong to: the web app can talk to one (approve a step, stop it) by its number.
    pub streams: Mutex<HashMap<u64, (String, UnixStream)>>,
    pub next: AtomicU64,
    pub conns: AtomicUsize,
    /// The owner's details for the privacy notice and the terms.
    pub legal: pages::Legal,
    /// Whether a signed-in person must have accepted the current terms before anything else (on when there is a database).
    pub require_terms: std::sync::atomic::AtomicBool,
    /// People known to have accepted `pages::TERMS_VERSION` (so the database is read once per person).
    pub accepted: Mutex<std::collections::HashSet<String>>,
    /// Notifications when the browser is closed (`push.rs`).
    pub push: Arc<push::Hub>,
    /// The desktop link: which computers are online, the jobs on their way to them (`link.rs`).
    pub link: link::Link,
    /// The shells of the cloud terminal (`term.rs`) and the folder their homes are made in.
    pub term: term::Hub,
    pub term_root: std::path::PathBuf,
}

impl App {
    /// Has this person accepted the current terms? Read from their record in the database (and remembered).
    pub fn has_accepted(&self, tenant: &str) -> Result<bool, String> {
        if !self.require_terms.load(Ordering::SeqCst) {
            return Ok(true);
        }
        if self.accepted.lock().unwrap().contains(tenant) {
            return Ok(true);
        }
        let Some(db) = self.tenants.db() else { return Ok(true) };
        let yes = db.get_doc(&devices::list_collection(tenant), "terms")?.and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.get("version").and_then(Value::as_str).map(|s| s == pages::TERMS_VERSION)).unwrap_or(false);
        if yes {
            self.accepted.lock().unwrap().insert(tenant.to_string());
        }
        Ok(yes)
    }

    pub fn close_streams(&self, tenant: &str) {
        self.link.purge_tenant(tenant);
        self.term.close_all(tenant);
        for (_, (t, s)) in self.streams.lock().unwrap().iter() {
            if t == tenant {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("header")
}

fn cors(app: &App) -> Vec<Header> {
    if app.no_auth {
        return Vec::new(); // no cross-site access: only the page the server itself serves
    }
    vec![
        header("Access-Control-Allow-Origin", "*"),
        header("Access-Control-Allow-Headers", "Authorization, Content-Type, X-ZO-Local"),
        header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        header("Access-Control-Max-Age", "86400"),
    ]
}

/// Headers every answer carries.
fn secure(app: &App) -> Vec<Header> {
    let mut h = vec![header("X-Content-Type-Options", "nosniff"), header("Referrer-Policy", "no-referrer")];
    if !app.no_auth {
        h.push(header("Strict-Transport-Security", "max-age=31536000"));
    }
    h
}

/// An answer without any cross-site permission.
fn send_bare(app: &App, req: Request, mut r: Response<std::io::Cursor<Vec<u8>>>) {
    for h in secure(app) {
        r.add_header(h);
    }
    let _ = req.respond(r);
}

fn send_response(app: &App, req: Request, mut r: Response<std::io::Cursor<Vec<u8>>>) {
    for h in cors(app).into_iter().chain(secure(app)) {
        r.add_header(h);
    }
    let _ = req.respond(r);
}

fn json_response(status: u16, body: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body.to_string()).with_status_code(status).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store"))
}

fn reply(app: &App, req: Request, status: u16, body: Value) {
    send_response(app, req, json_response(status, body));
}

fn text(app: &App, req: Request, status: u16, body: &str) {
    send_response(app, req, Response::from_string(body).with_status_code(status).with_header(header("Cache-Control", "no-store")));
}

fn html(app: &App, req: Request, body: String, csp: &str) {
    let r = Response::from_string(body)
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
        .with_header(header("Cache-Control", "no-cache"))
        .with_header(header("Content-Security-Policy", csp))
        .with_header(header("X-Frame-Options", "DENY"))
        // The page itself may use the microphone (voice input); frames and cameras stay off.
        .with_header(header("Permissions-Policy", "microphone=(self), camera=()"));
    send_response(app, req, r);
}

/// Why a request is not served.
#[derive(Debug)]
pub struct Fail {
    pub status: u16,
    pub error: String,
    pub retry: Option<u64>,
}

impl Fail {
    pub(crate) fn new(status: u16, error: impl Into<String>) -> Fail {
        Fail { status, error: error.into(), retry: None }
    }
}

impl From<Refusal> for Fail {
    fn from(r: Refusal) -> Fail {
        match r {
            Refusal::Rate(s) => Fail { status: 429, error: "You are sending requests too fast. Wait a moment.".into(), retry: Some(s) },
            Refusal::Busy(m, s) => Fail { status: 429, error: m.into(), retry: Some(s) },
            Refusal::Full => Fail::new(503, "Zohara Agent has as many people as it can take right now. Try again later."),
            Refusal::Gone => Fail::new(410, "This account was deleted."),
            Refusal::Storage(e) => Fail::new(503, e),
        }
    }
}

fn fail_response(f: Fail) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({"ok": false, "error": f.error, "retry_after": f.retry});
    let mut r = json_response(f.status, body);
    if let Some(s) = f.retry {
        r.add_header(header("Retry-After", &s.to_string()));
    }
    r
}

fn fail(app: &App, req: Request, f: Fail) {
    send_response(app, req, fail_response(f));
}

fn get_header(req: &Request, name: &'static str) -> Option<String> {
    req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string())
}

/// The no-login test mode has no password, so it must only answer the page it serves itself, on this computer. A web page from
/// another site cannot add a custom header without the server's permission (which is never given), and a name that points at
/// this computer from elsewhere (DNS rebinding) has another Host, so both are refused.
fn local_only(req: &Request) -> Result<(), String> {
    if get_header(req, "X-ZO-Local").as_deref() != Some("1") {
        return Err("The test mode only answers its own page.".into());
    }
    let host = get_header(req, "Host").unwrap_or_default().to_lowercase();
    if !host_is_local(&host) {
        return Err("The test mode only answers requests addressed to this computer.".into());
    }
    if let Some(origin) = get_header(req, "Origin") {
        if origin.split_once("://").map(|(_, h)| h.to_lowercase()).as_deref() != Some(host.as_str()) {
            return Err("The test mode does not answer other web pages.".into());
        }
    }
    Ok(())
}

/// `127.0.0.1`, `localhost` or `[::1]`, with or without a port.
fn host_is_local(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix("[::1]") {
        if rest.is_empty() || rest.starts_with(':') { "[::1]" } else { "" }
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(name, "127.0.0.1" | "localhost" | "[::1]")
}

/// The one person of the no-login test mode.
pub fn local_tenant() -> String {
    auth::tenant_id("zo-no-auth-local-person")
}

/// Where a request comes from: the address the host's proxy saw (the last entry of X-Forwarded-For, which a client cannot forge),
/// else the connection's own.
fn source(req: &Request) -> String {
    get_header(req, "X-Forwarded-For").and_then(|v| v.rsplit(',').next().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).or_else(|| req.remote_addr().map(|a| a.ip().to_string())).unwrap_or_else(|| "-".into())
}

/// Who is asking, or why they are refused. A Firebase token makes (or finds) a person; a device token finds the person who made it.
fn who(app: &App, req: &Request, path: &str) -> Result<Who, Fail> {
    if app.no_auth {
        local_only(req).map_err(|e| Fail::new(401, e))?;
        return Ok(Who { cred: Cred::Account { uid: "local".into(), email: String::new() }, tenant: local_tenant(), admin: app.noauth_admin });
    }
    let token = get_header(req, "Authorization").and_then(|v| v.strip_prefix("Bearer ").map(|t| t.trim().to_string())).ok_or_else(|| Fail::new(401, "Sign in first."))?;
    if token.starts_with("zd_") {
        let Some(devs) = &app.devices else { return Err(Fail::new(401, "The device token is not right.")) };
        let r = devs.resolve_from(&token, &source(req)).map_err(|m| match m {
            Miss::Unknown => Fail::new(401, "The device token is not right (or it was revoked)."),
            Miss::Rate(s) => Fail { status: 429, error: "Too many wrong tokens. Wait a moment.".into(), retry: Some(s) },
            Miss::Storage(e) => {
                eprintln!("zo-server: {e}");
                Fail::new(503, "The storage is not reachable right now. Try again in a moment.")
            }
        })?;
        if app.policy.is_blocked_hash(&r.eh) {
            return Err(Fail::new(403, "This account is blocked."));
        }
        return Ok(Who { cred: Cred::Device { scope: r.scope, id: r.id }, tenant: r.tenant, admin: !r.eh.is_empty() && app.policy.is_admin_hash(&r.eh) });
    }
    let verifier = app.verifier.as_ref().ok_or_else(|| Fail::new(401, "Sign in first."))?;
    let id = verifier.verify(&token).map_err(|e| Fail::new(401, e))?;
    // a blocked address is refused everywhere except taking its own data away or deleting it: that is everybody's right
    if app.policy.is_blocked(&id.email) && !matches!(path, "/api/account/export" | "/api/account/delete") {
        return Err(Fail::new(403, "This account is blocked."));
    }
    let admin = app.policy.is_admin(&id.email) && !app.policy.is_blocked(&id.email);
    Ok(Who { cred: Cred::Account { uid: id.uid.clone(), email: id.email }, tenant: auth::tenant_id(&id.uid), admin })
}

/// How long a request may take to send its body (the sockets also drop a client that is silent for 20 s, see vendor/tiny_http).
const BODY_SECS: u64 = 30;
const SYNC_BODY_SECS: u64 = 120;
/// 14 MB over a slow phone connection: more time than a plain message gets, but not unlimited.
const SEND_BODY_SECS: u64 = 90;
/// The sign-in calls are small, and the two that need no login give a client even less time.
const SMALL_BODY_LIMIT: usize = 4096;
const PUBLIC_BODY_SECS: u64 = 10;

fn body(req: &mut Request, limit: usize, secs: u64) -> Result<Value, Fail> {
    if req.body_length().is_some_and(|n| n > limit) {
        return Err(Fail::new(413, "The request is too big."));
    }
    // read in pieces against a deadline: a client that trickles one byte at a time cannot hold the thread for ever
    let (t0, mut raw, mut buf) = (Instant::now(), Vec::new(), [0u8; 16384]);
    let reader = req.as_reader();
    loop {
        if t0.elapsed() > Duration::from_secs(secs) {
            return Err(Fail::new(408, "The request took too long to arrive."));
        }
        let want = buf.len().min(limit + 1 - raw.len());
        match reader.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Err(Fail::new(408, "The request took too long to arrive.")),
            Err(_) => return Err(Fail::new(400, "The request could not be read.")),
        }
        if raw.len() > limit {
            return Err(Fail::new(413, "The request is too big."));
        }
    }
    let s = String::from_utf8(raw).map_err(|_| Fail::new(400, "The request is not text."))?;
    if s.trim().is_empty() {
        return Ok(json!({}));
    }
    let v: Value = serde_json::from_str(&s).map_err(|_| Fail::new(400, "The request is not JSON."))?;
    if v.is_object() { Ok(v) } else { Err(Fail::new(400, "The request must be a JSON object.")) }
}

fn connect() -> Result<UnixStream, String> {
    zo_core::service::connect().map_err(|e| format!("Zohara Agent's service is not running: {e}"))
}

/// The request as the service gets it: whose it is comes from the verified credential, whatever the client wrote.
fn for_service(mut v: Value, t: &zo_core::tenant::Tenant) -> Value {
    v["tenant"] = json!({"id": t.id, "restricted": t.restricted});
    v
}

/// The ops that call a model (or its provider) and so take as long as an answer.
pub fn op_uses_model(name: &str) -> bool {
    matches!(name, "compact" | "memory_edit" | "personality_generate" | "models" | "model_test")
}

/// One question, one answer (everything except sending a message).
fn op(v: &Value) -> Result<Value, String> {
    let mut conn = connect()?;
    conn.set_read_timeout(Some(Duration::from_secs(180))).ok();
    conn.write_all(format!("{v}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(conn).read_line(&mut line).map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|_| "Zohara Agent gave no answer.".into())
}

/// Sends a message and passes the answer on as it is written: one JSON object per line. The lease and the permit are held until
/// the answer is over, so a deleted account waits for it and the limits count it.
fn send(app: &Arc<App>, req: Request, v: Value, lease: Lease, tenant: zo_core::tenant::Tenant) {
    let permit = match lease.permit_send() {
        Ok(p) => p,
        Err(r) => return fail(app, req, r.into()),
    };
    // "keep_running": the answer is finished even if the browser goes away (then a push says so). Only for a person who has a device
    // to notify; for everybody else the field is dropped and a closed connection stops the answer as before.
    let keep = v.get("keep_running").and_then(Value::as_bool) == Some(true) && app.push.has_any(&tenant.id);
    let mut v = for_service(v, &tenant);
    v["op"] = json!("send");
    if let Some(o) = v.as_object_mut() {
        o.remove("keep_running");
        if keep {
            o.insert("keep_running".into(), json!(true));
        }
    }
    let conn = match connect() {
        Ok(c) => c,
        Err(e) => return reply(app, req, 503, json!({"ok": false, "error": e})),
    };
    let mut to_service = match conn.try_clone() {
        Ok(c) => c,
        Err(e) => return reply(app, req, 500, json!({"ok": false, "error": e.to_string()})),
    };
    if to_service.write_all(format!("{v}\n").as_bytes()).is_err() {
        return reply(app, req, 503, json!({"ok": false, "error": "Zohara Agent's service closed the connection."}));
    }
    let id = app.next.fetch_add(1, Ordering::SeqCst);
    app.streams.lock().unwrap().insert(id, (tenant.id.clone(), to_service));
    let mut out = req.into_writer();
    let cors = if app.no_auth { "" } else { "Access-Control-Allow-Origin: *\r\n" };
    let head = &format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Accel-Buffering: no\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n{cors}\r\n");
    let chunk = |out: &mut Box<dyn Write + Send>, line: &str| -> bool { out.write_all(format!("{:x}\r\n{line}\r\n", line.len()).as_bytes()).is_ok() && out.flush().is_ok() };
    let mut ok = out.write_all(head.as_bytes()).is_ok() && chunk(&mut out, &format!("{}\n", json!({"event": "stream", "id": id})));
    let mut reader = BufReader::new(conn);
    let mut line = String::new();
    // what the push needs to know, read from the lines that pass: the chat, and whether there is an answer to tell about
    let (mut chat_id, mut chat_title, mut answered, mut stopped) = (String::new(), String::new(), false, false);
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    loop {
        if !ok && !keep {
            break;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if keep {
                    if let Ok(e) = serde_json::from_str::<Value>(&line) {
                        match e.get("event").and_then(Value::as_str) {
                            Some("chat") => {
                                chat_id = e.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                                chat_title = e.get("title").and_then(Value::as_str).unwrap_or("").to_string();
                            }
                            Some("text") | Some("delta") => answered = true,
                            Some("stopped") => stopped = true,
                            _ => {}
                        }
                    }
                }
                if ok {
                    ok = chunk(&mut out, &line);
                    if !ok && keep {
                        // the person went away: the answer carries on, within the usual limit, then a push says it is ready
                        if let Some((_, s)) = app.streams.lock().unwrap().get_mut(&id) {
                            let _ = s.write_all(b"{\"op\":\"detached\"}\n");
                        }
                        let (app, f) = (app.clone(), finished.clone());
                        let limit = if tenant.restricted { KEEP_RESTRICTED_SECS } else { KEEP_ADMIN_SECS };
                        std::thread::spawn(move || {
                            let t0 = Instant::now();
                            while !f.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(limit) {
                                std::thread::sleep(Duration::from_millis(500));
                            }
                            if !f.load(Ordering::SeqCst) {
                                if let Some((_, s)) = app.streams.lock().unwrap().get(&id) {
                                    let _ = s.shutdown(std::net::Shutdown::Both);
                                }
                            }
                        });
                    }
                }
            }
        }
    }
    finished.store(true, Ordering::SeqCst);
    let client_gone = !ok;
    if ok {
        let _ = out.write_all(b"0\r\n\r\n");
        let _ = out.flush();
    }
    // closing the connection tells the service to stop if the person went away
    if let Some((_, s)) = app.streams.lock().unwrap().remove(&id) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    drop(permit);
    drop(lease);
    // the permits are free again; a person who stayed connected has seen the answer, so only a person who left is told
    if keep && client_gone && answered && !stopped {
        app.push.notify_answer(&tenant.id, &chat_id, &chat_title);
    }
}

fn route(app: Arc<App>, mut req: Request) {
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    // The two calls a device makes before it is signed in are for programs (the app, `zo`), never for another web page: no
    // cross-site permission at all, and a request that carries an Origin (only browsers send one) is refused. Otherwise any page could
    // start a sign-in from its visitor's own address and show the visitor the code.
    if matches!(path.as_str(), "/api/auth/device_start" | "/api/auth/device_poll") && (method == Method::Options || get_header(&req, "Origin").is_some()) {
        let r = Response::from_string(json!({"ok": false, "error": "This call is for the Zohara Agent app and terminal client, not for web pages."}).to_string()).with_status_code(403).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store"));
        return send_bare(&app, req, r);
    }
    match (&method, path.as_str()) {
        (Method::Options, _) => return text(&app, req, 204, ""),
        (Method::Get | Method::Head, "/healthz") => return text(&app, req, 200, "ok"),
        // /device is the approval page: a page of the same app (it works out from the address which screen to show)
        (Method::Get | Method::Head, "/") | (Method::Get | Method::Head, "/index.html") | (Method::Get | Method::Head, "/device") => return html(&app, req, pwa::index_for(&app), pages::APP_CSP),
        // the installable app: manifest, icons and the service worker (see pwa.rs)
        (Method::Get | Method::Head, p) if pwa::serves(p) => return pwa::serve(&app, req, p),
        // Google Search Console ownership verification file (must stay here permanently)
        (Method::Get | Method::Head, "/google0dfec91cc0809278.html") => {
            let r = Response::from_string("google-site-verification: google0dfec91cc0809278.html")
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-cache"));
            return send_response(&app, req, r);
        }
        (Method::Get, "/privacy") => return html(&app, req, pages::page("Zohara Agent privacy notice", &pages::fill(pages::PRIVACY_MD, &app.legal)), pages::PLAIN_CSP),
        (Method::Get, "/terms") => return html(&app, req, pages::page("Zohara Agent terms of use", &pages::fill(pages::TERMS_MD, &app.legal)), pages::PLAIN_CSP),
        _ => {}
    }
    let Some(r) = route_of(&method, &path) else { return text(&app, req, 404, "not found") };
    let ip = source(&req);
    // a device that is not signed in yet: nothing to check but where it comes from, and a small body read against a short deadline
    if matches!(r, Route::DeviceStart | Route::DevicePoll) {
        let ip = source(&req);
        if app.no_auth {
            return fail(&app, req, Fail::new(400, "Signing in a device needs the cloud database, which the test mode does not have."));
        }
        let v = match body(&mut req, SMALL_BODY_LIMIT, PUBLIC_BODY_SECS) {
            Ok(v) => v,
            Err(f) => return fail(&app, req, f),
        };
        return match loginapi::public(&app, path.strip_prefix("/api/auth/").unwrap_or(""), &v, &ip) {
            Ok(r) => send_bare(&app, req, json_response(200, r)),
            Err(f) => send_bare(&app, req, fail_response(f)),
        };
    }
    let who = match who(&app, &req, &path) {
        Ok(w) => w,
        Err(f) => return fail(&app, req, f),
    };
    if !permits(&who.cred, r) {
        return fail(&app, req, Fail::new(403, "This device token may not do that."));
    }
    // terminals and the Claude sign-in: small calls and one long stream; they hold no place of the person's (a stream would hold it for
    // hours) and have their own limits (term.rs, claude_api.rs)
    if matches!(r, Route::Term | Route::Claude) {
        return term_or_claude(&app, req, &path, r, &who);
    }
    // the desktop app's long poll and its event posts: they have their own limits (link.rs) and hold no place of the person's, which
    // would otherwise be taken up for as long as a poll waits
    if r == Route::LinkDevice {
        let Cred::Device { id, .. } = &who.cred else { return fail(&app, req, Fail::new(403, "Only the desktop app's own credential may do that.")) };
        let is_poll = path == "/api/link/poll";
        let v = match body(&mut req, if is_poll { link::POLL_BODY_LIMIT } else { BODY_LIMIT }, BODY_SECS) {
            Ok(v) => v,
            Err(f) => return fail(&app, req, f),
        };
        let res = if is_poll { link::poll_http(&app, &who, id, &v) } else { link::events_http(&app, &who, id, &v) };
        return match res {
            Ok(r) => reply(&app, req, 200, r),
            // a job that is not (or no longer) this computer's: it must stop working on it
            Err(f) if !is_poll && f.status == 404 => reply(&app, req, 404, json!({"ok": false, "error": "unknown_job", "cancel": true})),
            Err(f) => fail(&app, req, f),
        };
    }
    let lease = match app.tenants.acquire(&who.tenant, who.admin, Instant::now()) {
        Ok(l) => l,
        Err(e) => return fail(&app, req, e.into()),
    };
    let tenant = lease.tenant(!who.admin);
    // somebody who signs in (not a device they made, which only exists after they accepted) must have accepted the current terms
    if matches!(who.cred, Cred::Account { .. }) {
        let free = (r == Route::Account && matches!(path.as_str(), "/api/account/whoami" | "/api/account/accept_terms" | "/api/account/export" | "/api/account/delete")) || r == Route::Signout;
        if !free {
            match app.has_accepted(&who.tenant) {
                Ok(true) => {}
                Ok(false) => {
                    let body = json!({"ok": false, "error": "Accept the privacy notice and the terms first.", "terms_required": true, "version": pages::TERMS_VERSION});
                    return reply(&app, req, 403, body);
                }
                Err(e) => {
                    eprintln!("zo-server: {e}");
                    return fail(&app, req, Fail::new(503, "The storage is not reachable right now. Try again in a moment."));
                }
            }
        }
    }
    // a sync body is big and is held in memory: only a few at a time. A model-calling op takes the same place as an answer.
    let _sync = if r == Route::Sync {
        match lease.permit_sync() {
            Ok(p) => Some(p),
            Err(e) => return fail(&app, req, e.into()),
        }
    } else {
        None
    };
    let limit = match r {
        Route::Sync => SYNC_BODY_LIMIT,
        Route::Send => SEND_BODY_LIMIT,
        Route::Auth | Route::Signout | Route::LinkStatus | Route::ClaudeToken => SMALL_BODY_LIMIT,
        _ => BODY_LIMIT,
    };
    let secs = match r {
        Route::Sync => SYNC_BODY_SECS,
        Route::Send => SEND_BODY_SECS,
        _ => BODY_SECS,
    };
    // a message body may be 14 MB and is held in memory while it arrives: only a few big ones at a time (a plain message is small)
    let _big = if r == Route::Send && req.body_length().is_none_or(|n| n > BODY_LIMIT) {
        let busy = || Fail { status: 429, error: "Too many large messages are being sent right now. Try again in a moment.".into(), retry: Some(5) };
        // the person's own share first (so one account cannot take every place), then the global one
        let Some(mine) = BigShare::take(&who.tenant) else { return fail(&app, req, busy()) };
        match BigSend::take(&BIG_SENDS) {
            Some(g) => Some((mine, g)),
            None => return fail(&app, req, busy()),
        }
    } else {
        None
    };
    let mut v = match body(&mut req, limit, secs) {
        Ok(v) => v,
        Err(f) => return fail(&app, req, f),
    };
    drop(_big);
    if r == Route::Send {
        // strict checks before anything is passed on (the service checks again): what goes on is rebuilt from the checked fields only
        match zo_core::attach::parse(v.get("attachments")) {
            Ok(a) if a.is_empty() => {
                v.as_object_mut().map(|o| o.remove("attachments"));
            }
            Ok(a) => v["attachments"] = a.to_json(),
            Err(e) => return reply(&app, req, 400, json!({"ok": false, "error": e})),
        }
    }
    match r {
        // "target": "desktop": the message runs on the person's own computer through the desktop link
        Route::Send if v.get("target").and_then(Value::as_str) == Some("desktop") => {
            drop(lease); // the relay may take an hour: it keeps no place of the person's, deleting the account ends it
            link::send_http(&app, req, v, &who)
        }
        Route::Send => send(&app, req, v, lease, tenant),
        Route::Sync => match sync_api::handle(&path, &v, &tenant, matches!(who.cred, Cred::Device { scope: Scope::Sync, .. })) {
            Ok(r) => reply(&app, req, 200, r),
            Err(e) => reply(&app, req, 400, json!({"ok": false, "error": e})),
        },
        Route::Reply => {
            // a line for an answer that is running: {"stream": 3, "op": "stop"} or an approval; only the person's own answers
            let id = v.get("stream").and_then(Value::as_u64).unwrap_or(0);
            let mut v = v;
            if let Some(o) = v.as_object_mut() {
                o.remove("tenant");
            }
            let local = match app.streams.lock().unwrap().get_mut(&id) {
                Some((t, s)) if *t == tenant.id => Some(s.write_all(format!("{v}\n").as_bytes()).is_ok()),
                _ => None,
            };
            match local {
                Some(sent) => reply(&app, req, if sent { 200 } else { 404 }, json!({"ok": sent})),
                // not an answer made on the server: one running on the person's computer (stop, or an approval)
                None => {
                    let (status, out) = link::reply_http(&app, &who, id, &v);
                    reply(&app, req, status, out)
                }
            }
        }
        Route::Auth | Route::Signout => match loginapi::handle(&app, path.strip_prefix("/api/auth/").unwrap_or(""), &v, &who, &ip) {
            Ok(r) => reply(&app, req, 200, r),
            Err(f) => fail(&app, req, f),
        },
        Route::DeviceStart | Route::DevicePoll | Route::LinkDevice | Route::Term | Route::Claude => fail(&app, req, Fail::new(500, "not here")),
        Route::LinkStatus => match link::status_http(&app, &who) {
            Ok(r) => reply(&app, req, 200, r),
            Err(f) => fail(&app, req, f),
        },
        Route::ClaudeToken => {
            let (status, out) = account::claude_token(&v, &who, &tenant);
            reply(&app, req, status, out)
        }
        Route::Account => {
            let name = path.strip_prefix("/api/account/").unwrap_or("");
            let (status, out) = account::handle(&app, name, &v, &who, &lease);
            reply(&app, req, status, out)
        }
        Route::Op => {
            if v.get("op").and_then(Value::as_str) == Some("send") {
                return reply(&app, req, 400, json!({"ok": false, "error": "Use /api/send to send a message."}));
            }
            // a settings question for the person's own computer (whitelisted, see link.rs); it waits for the answer, so no place is held
            if v.get("op").and_then(Value::as_str) == Some("desktop_op") {
                drop(lease);
                let (status, out) = link::op_http(&app, &who, &v);
                return reply(&app, req, status, out);
            }
            // these ask a model (up to minutes): they count as a running answer, like /api/send
            let _run = if v.get("op").and_then(Value::as_str).is_some_and(op_uses_model) {
                match lease.permit_send() {
                    Ok(p) => Some(p),
                    Err(e) => return fail(&app, req, e.into()),
                }
            } else {
                None
            };
            match op(&for_service(v, &tenant)) {
                Ok(r) => reply(&app, req, 200, r),
                Err(e) => reply(&app, req, 502, json!({"ok": false, "error": e})),
            }
        }
    }
}

/// Does the request come from this server's own page? Browsers send `Origin` on every POST: another site's page is refused. Programs
/// (`zo`, curl) send none. A terminal is the most powerful thing the API has, so this is checked on top of the sign-in.
fn same_origin(app: &App, req: &Request) -> bool {
    let Some(origin) = get_header(req, "Origin") else { return true };
    let Some(host) = get_header(req, "Host") else { return false };
    origin == format!("https://{host}") || (app.no_auth && origin == format!("http://{host}") && host_is_local(&host))
}

fn term_or_claude(app: &Arc<App>, mut req: Request, path: &str, r: Route, who: &Who) {
    if !same_origin(app, &req) {
        return fail(app, req, Fail::new(403, "This call is only for the Zohara Agent page of this server."));
    }
    if matches!(who.cred, Cred::Account { .. }) {
        match app.has_accepted(&who.tenant) {
            Ok(true) => {}
            Ok(false) => return reply(app, req, 403, json!({"ok": false, "error": "Accept the privacy notice and the terms first.", "terms_required": true, "version": pages::TERMS_VERSION})),
            Err(e) => {
                eprintln!("zo-server: {e}");
                return fail(app, req, Fail::new(503, "The storage is not reachable right now. Try again in a moment."));
            }
        }
    }
    // typed text can be pasted in bulk, and JSON escapes control characters at up to six bytes each
    let v = match body(&mut req, 512 * 1024, BODY_SECS) {
        Ok(v) => v,
        Err(f) => return fail(app, req, f),
    };
    if r == Route::Term {
        let name = path.strip_prefix("/api/term/").unwrap_or("");
        if name == "open" {
            return term::open_http(app, req, v, who);
        }
        return term::call_http(app, req, name, &v, who);
    }
    claude_api::handle(app, req, path.strip_prefix("/api/claude/").unwrap_or(""), &v, who)
}

/// Serves requests until the listener ends.
pub fn serve(app: Arc<App>, server: Server) {
    for req in server.incoming_requests() {
        if app.conns.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            app.conns.fetch_sub(1, Ordering::SeqCst);
            fail(&app, req, Fail { status: 503, error: "The server is busy. Try again in a moment.".into(), retry: Some(5) });
            continue;
        }
        let app = app.clone();
        std::thread::spawn(move || {
            struct Done(Arc<App>);
            impl Drop for Done {
                fn drop(&mut self) {
                    self.0.conns.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _d = Done(app.clone());
            route(app, req);
        });
    }
}

/// Empties the people's folders at start: Firestore is the record, and a leftover folder (an account deleted meanwhile, or changes
/// that were never sent) must not come back. Only folders that are named like a person's are touched.
fn clean_root(root: &std::path::Path) {
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        if e.file_name().to_str().is_some_and(zo_core::tenant::valid_id) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

fn main() {
    // first of all, before any thread exists: the secrets are read and taken out of the environment (see hardening.rs)
    let secrets = hardening::take_secrets();
    // a shell started on this server (an admin's terminal) must not be able to read this process's memory or environment through /proc
    hardening::protect_memory();
    zo_core::ai::keys::set_server_secret(secrets.server_secret.as_deref().unwrap_or(""));
    zo_core::ai::claude_code::set_global_token(secrets.claude_token.clone());
    let no_auth = env("ZO_SERVER_NO_AUTH").as_deref() == Some("1");
    let account = secrets.firebase.clone();
    if !no_auth && account.is_none() {
        eprintln!("zo-server: set FIREBASE_SERVICE_ACCOUNT (and ZO_ADMIN_EMAILS), or ZO_SERVER_NO_AUTH=1 to try it on your own computer.");
        std::process::exit(2);
    }
    let policy = Policy::new(env("ZO_ADMIN_EMAILS"), env("ZO_BLOCKED_EMAILS"), env("ZO_MAX_USERS"));
    if !no_auth && policy.admin_count() == 0 {
        eprintln!("zo-server: ZO_ADMIN_EMAILS is empty: nobody is an admin; everybody is restricted.");
    }
    // the privacy notice and the terms name the operator: a public service does not start without them
    let legal = pages::Legal { contact: env("ZO_CONTACT_EMAIL").unwrap_or_default(), region: env("ZO_HOSTING_REGION").unwrap_or_default(), country: env("ZO_OPERATOR_COUNTRY").unwrap_or_default() };
    if !no_auth && (legal.contact.is_empty() || legal.region.is_empty() || legal.country.is_empty()) {
        eprintln!("zo-server: set ZO_CONTACT_EMAIL, ZO_HOSTING_REGION and ZO_OPERATOR_COUNTRY: the privacy notice and the terms show them (people must be able to reach the operator).");
        std::process::exit(2);
    }
    let legal = if no_auth && legal.contact.is_empty() { pages::Legal { contact: "(test mode)".into(), region: "(test mode)".into(), country: "(test mode)".into() } } else { legal };
    // devices show people this address to sign in at: required whenever there is a login (a bad value is an error, not ignored)
    let public_url = if no_auth {
        None
    } else {
        match env("ZO_PUBLIC_URL").or_else(|| env("RENDER_EXTERNAL_URL")) {
            Some(u) => match devicelogin::parse_public_base(&u) {
                Ok(b) => Some(b),
                Err(e) => {
                    eprintln!("zo-server: ZO_PUBLIC_URL / RENDER_EXTERNAL_URL: {e}");
                    std::process::exit(2);
                }
            },
            None => {
                eprintln!("zo-server: set ZO_PUBLIC_URL (for example https://zo-ai-server.onrender.com): devices send people to <it>/device to sign in.");
                std::process::exit(2);
            }
        }
    };
    let max_runs = env("ZO_MAX_RUNS").and_then(|m| m.parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or(8);

    // from here on zo-core serves many people: every request must name its tenant, and the default folders are never used
    let root = std::path::PathBuf::from(env("ZO_TENANT_ROOT").unwrap_or_else(|| "/tmp/zo-tenants".into()));
    {
        use std::os::unix::fs::DirBuilderExt;
        if let Err(e) = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&root) {
            eprintln!("zo-server: cannot make {}: {e}", root.display());
            std::process::exit(2);
        }
    }
    std::env::set_var("ZO_MULTI_TENANT", "1");
    std::env::set_var("ZO_TENANT_ROOT", &root);
    let secret = secrets.server_secret.clone();
    if secret.is_none() && !no_auth {
        eprintln!("zo-server: ZO_SERVER_SECRET is not set: nobody can save an API key here.");
    }

    let (mut verifier, mut database): (Option<Arc<dyn Verifier>>, Option<Arc<dyn db::Db>>) = (None, None);
    if let (Some(account), false) = (account, no_auth) {
        let fs = match firestore::Firestore::new(&account) {
            Ok(fs) => fs,
            Err(e) => {
                eprintln!("zo-server: {e}");
                std::process::exit(2);
            }
        };
        verifier = Some(Arc::new(auth::Firebase { project: env("FIREBASE_PROJECT_ID").unwrap_or_else(|| fs.project().to_string()) }));
        database = Some(Arc::new(fs));
        clean_root(&root);
    }
    let root_for_terminals = root.clone();
    let tenants = Arc::new(TenantMap::new(root, database.clone(), secret.clone().unwrap_or_default(), policy.max_users, max_runs));
    // never start without the database and then overwrite what is saved
    if let Some(reg) = tenants.registry() {
        match reg.count() {
            Ok(n) => eprintln!("zo-server: Firestore is reachable; {n} people registered (at most {})", policy.max_users),
            Err(e) => {
                eprintln!("zo-server: cannot read Firestore, so the server does not start: {e}");
                std::process::exit(3);
            }
        }
    }
    {
        let t = tenants.clone();
        std::thread::spawn(move || {
            let mut last_evict = Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(3));
                if t.push_all() > 0 {
                    std::thread::sleep(Duration::from_secs(20));
                }
                if last_evict.elapsed() > Duration::from_secs(60) {
                    last_evict = Instant::now();
                    let n = t.evict_idle(Instant::now());
                    if n > 0 {
                        eprintln!("zo-server: {n} idle people taken off the disk");
                    }
                }
            }
        });
    }

    if let Err(e) = zo_core::service::start() {
        eprintln!("zo-server: {e}");
        std::process::exit(1);
    }

    // Render puts a free server to sleep when nobody calls it: call ourselves every few minutes
    if let Some(url) = env("RENDER_EXTERNAL_URL").or_else(|| env("ZO_SELF_URL")) {
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(8 * 60));
            if let Err(e) = ureq::get(&format!("{}/healthz", url.trim_end_matches('/'))).timeout(Duration::from_secs(30)).call() {
                eprintln!("zo-server: the self-ping failed: {e}");
            }
        });
    }

    let port = env("PORT").unwrap_or_else(|| "8787".into());
    let host = if no_auth { "127.0.0.1" } else { "0.0.0.0" };
    let addr = format!("{host}:{port}");
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zo-server: cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("zo-server: listening on {addr}{}", if no_auth { " (no login: testing only)" } else { "" });
    let devices = database.clone().map(Devices::new);
    // the contact in the notification key's token: push services write to it if the server misbehaves
    let subject = if legal.contact.contains('@') && !legal.contact.contains(char::is_whitespace) { format!("mailto:{}", legal.contact) } else { public_url.clone().unwrap_or_else(|| "mailto:nobody@invalid.example".into()) };
    let push = Arc::new(push::Hub::new(database.clone(), secret.as_deref().unwrap_or(""), subject));
    let app = Arc::new(App {
        no_auth,
        noauth_admin: no_auth && env("ZO_NOAUTH_ADMIN").as_deref() == Some("1"),
        verifier,
        policy,
        tenants,
        devices,
        login: DeviceLogin::new(Default::default()),
        public_url,
        streams: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        conns: AtomicUsize::new(0),
        legal,
        require_terms: std::sync::atomic::AtomicBool::new(!no_auth),
        accepted: Mutex::new(Default::default()),
        push,
        link: link::Link::default(),
        term: term::Hub::default(),
        term_root: root_for_terminals,
    });
    {
        // requests that ran out are dropped, and a credential nobody collected is revoked
        let a = app.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            loginapi::sweep(&a);
            a.link.sweep();
            a.term.sweep();
        });
    }
    serve(app, server);
}

#[cfg(test)]
mod tests;
