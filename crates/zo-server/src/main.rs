//! ZO-AI in the cloud, as a public service: anybody with a Google account may sign in and use the same brain as the desktop app,
//! each person in their own isolated corner (a "tenant") with their own API key, their data kept in Firestore because the server's
//! own disk does not last. Only the owner's addresses are admins; everybody else is restricted to a small set of safe tools.
//!
//! Settings (environment):
//!   PORT                       where to listen (Render sets it)
//!   FIREBASE_SERVICE_ACCOUNT   the service account key file's contents (JSON): turns on the login check and Firestore (required)
//!   ZO_ADMIN_EMAILS            the owner's address(es), separated by commas: admins (unrestricted tools, Claude Code, SSH, Bypass)
//!   ZO_BLOCKED_EMAILS          addresses that are refused (optional)
//!   ZO_MAX_USERS               how many people may register (default 200)
//!   ZO_MAX_RUNS                answers being written at once on the whole server (default 8)
//!   ZO_SERVER_SECRET           a long random text: each person's API keys are encrypted with a key derived from it
//!   ZO_TENANT_ROOT             where the people's folders live on the disk (default /tmp/zo-tenants; emptied at start, Firestore is the record)
//!   RENDER_EXTERNAL_URL        the server's own address (Render sets it): it calls itself every few minutes so it is not put to sleep
//!   ZO_SERVER_NO_AUTH=1        for testing on your own computer only: no login, listens on 127.0.0.1 only, no Firestore, one fixed
//!                              (restricted) person; ZO_NOAUTH_ADMIN=1 makes that person an admin

mod account;
mod auth;
mod db;
mod devices;
mod firestore;
mod pages;
mod sync;
mod sync_api;
mod tenants;
#[cfg(test)]
mod testenv;

use auth::{Policy, Verifier};
use devices::{Devices, Miss, Scope};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tenants::{Lease, Refusal, TenantMap};
use tiny_http::{Header, Method, Request, Response, Server};

/// Request bodies: /api/op, /api/send and /api/reply, and the desktop sync.
const BODY_LIMIT: usize = 1_000_000;
const SYNC_BODY_LIMIT: usize = 20_000_000;
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
}

/// How a caller proved who they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cred {
    /// A Firebase sign-in (or the local test person).
    Account { uid: String, email: String },
    Device { scope: Scope },
}

#[derive(Clone, Debug)]
pub struct Who {
    pub cred: Cred,
    pub tenant: String,
    pub admin: bool,
}

/// Which routes a credential may call. A signed-in person: all. A `full` device: the API and the sync. A `sync` device: the sync.
pub fn permits(cred: &Cred, route: Route) -> bool {
    match cred {
        Cred::Account { .. } => true,
        Cred::Device { scope: Scope::Full } => route != Route::Account,
        Cred::Device { scope: Scope::Sync } => route == Route::Sync,
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
        p => p.strip_prefix("/api/account/").filter(|n| account::CALLS.contains(n)).map(|_| Route::Account),
    }
}

pub struct App {
    pub no_auth: bool,
    pub noauth_admin: bool,
    pub verifier: Option<Arc<dyn Verifier>>,
    pub policy: Policy,
    pub tenants: Arc<TenantMap>,
    pub devices: Option<Devices>,
    /// Answers running right now, with the person they belong to: the web app can talk to one (approve a step, stop it) by its number.
    pub streams: Mutex<HashMap<u64, (String, UnixStream)>>,
    pub next: AtomicU64,
    pub conns: AtomicUsize,
}

impl App {
    pub fn close_streams(&self, tenant: &str) {
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

fn send_response(app: &App, req: Request, mut r: Response<std::io::Cursor<Vec<u8>>>) {
    for h in cors(app).into_iter().chain(secure(app)) {
        r.add_header(h);
    }
    let _ = req.respond(r);
}

fn reply(app: &App, req: Request, status: u16, body: Value) {
    let r = Response::from_string(body.to_string()).with_status_code(status).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store"));
    send_response(app, req, r);
}

fn text(app: &App, req: Request, status: u16, body: &str) {
    send_response(app, req, Response::from_string(body).with_status_code(status).with_header(header("Cache-Control", "no-store")));
}

fn html(app: &App, req: Request, body: String, csp: &str) {
    let r = Response::from_string(body)
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
        .with_header(header("Cache-Control", "no-cache"))
        .with_header(header("Content-Security-Policy", csp))
        .with_header(header("X-Frame-Options", "DENY"));
    send_response(app, req, r);
}

/// Why a request is not served.
pub struct Fail {
    pub status: u16,
    pub error: String,
    pub retry: Option<u64>,
}

impl Fail {
    fn new(status: u16, error: impl Into<String>) -> Fail {
        Fail { status, error: error.into(), retry: None }
    }
}

impl From<Refusal> for Fail {
    fn from(r: Refusal) -> Fail {
        match r {
            Refusal::Rate(s) => Fail { status: 429, error: "You are sending requests too fast. Wait a moment.".into(), retry: Some(s) },
            Refusal::Busy(m, s) => Fail { status: 429, error: m.into(), retry: Some(s) },
            Refusal::Full => Fail::new(503, "ZO-AI has as many people as it can take right now. Try again later."),
            Refusal::Gone => Fail::new(410, "This account was deleted."),
            Refusal::Storage(e) => Fail::new(503, e),
        }
    }
}

fn fail(app: &App, req: Request, f: Fail) {
    let body = json!({"ok": false, "error": f.error, "retry_after": f.retry});
    let mut r = Response::from_string(body.to_string()).with_status_code(f.status).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store"));
    if let Some(s) = f.retry {
        r.add_header(header("Retry-After", &s.to_string()));
    }
    send_response(app, req, r);
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

/// Who is asking, or why they are refused. A Firebase token makes (or finds) a person; a device token finds the person who made it.
fn who(app: &App, req: &Request) -> Result<Who, Fail> {
    if app.no_auth {
        local_only(req).map_err(|e| Fail::new(401, e))?;
        return Ok(Who { cred: Cred::Account { uid: "local".into(), email: String::new() }, tenant: local_tenant(), admin: app.noauth_admin });
    }
    let token = get_header(req, "Authorization").and_then(|v| v.strip_prefix("Bearer ").map(|t| t.trim().to_string())).ok_or_else(|| Fail::new(401, "Sign in first."))?;
    if token.starts_with("zd_") {
        let Some(devs) = &app.devices else { return Err(Fail::new(401, "The device token is not right.")) };
        let r = devs.resolve(&token).map_err(|m| match m {
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
        return Ok(Who { cred: Cred::Device { scope: r.scope }, tenant: r.tenant, admin: !r.eh.is_empty() && app.policy.is_admin_hash(&r.eh) });
    }
    let verifier = app.verifier.as_ref().ok_or_else(|| Fail::new(401, "Sign in first."))?;
    let id = verifier.verify(&token).map_err(|e| Fail::new(401, e))?;
    if app.policy.is_blocked(&id.email) {
        return Err(Fail::new(403, "This account is blocked."));
    }
    let admin = app.policy.is_admin(&id.email);
    Ok(Who { cred: Cred::Account { uid: id.uid.clone(), email: id.email }, tenant: auth::tenant_id(&id.uid), admin })
}

fn body(req: &mut Request, limit: usize) -> Result<Value, Fail> {
    if req.body_length().is_some_and(|n| n > limit) {
        return Err(Fail::new(413, "The request is too big."));
    }
    let mut s = String::new();
    req.as_reader().take(limit as u64 + 1).read_to_string(&mut s).map_err(|_| Fail::new(400, "The request could not be read."))?;
    if s.len() > limit {
        return Err(Fail::new(413, "The request is too big."));
    }
    if s.trim().is_empty() {
        return Ok(json!({}));
    }
    let v: Value = serde_json::from_str(&s).map_err(|_| Fail::new(400, "The request is not JSON."))?;
    if v.is_object() { Ok(v) } else { Err(Fail::new(400, "The request must be a JSON object.")) }
}

fn connect() -> Result<UnixStream, String> {
    UnixStream::connect(zo_core::service::socket_path()).map_err(|e| format!("ZO-AI's service is not running: {e}"))
}

/// The request as the service gets it: whose it is comes from the verified credential, whatever the client wrote.
fn for_service(mut v: Value, t: &zo_core::tenant::Tenant) -> Value {
    v["tenant"] = json!({"id": t.id, "restricted": t.restricted});
    v
}

/// One question, one answer (everything except sending a message).
fn op(v: &Value) -> Result<Value, String> {
    let mut conn = connect()?;
    conn.set_read_timeout(Some(Duration::from_secs(180))).ok();
    conn.write_all(format!("{v}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(conn).read_line(&mut line).map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|_| "ZO-AI gave no answer.".into())
}

/// Sends a message and passes the answer on as it is written: one JSON object per line. The lease and the permit are held until
/// the answer is over, so a deleted account waits for it and the limits count it.
fn send(app: &Arc<App>, req: Request, v: Value, lease: Lease, tenant: zo_core::tenant::Tenant) {
    let permit = match lease.permit_send() {
        Ok(p) => p,
        Err(r) => return fail(app, req, r.into()),
    };
    let mut v = for_service(v, &tenant);
    v["op"] = json!("send");
    let conn = match connect() {
        Ok(c) => c,
        Err(e) => return reply(app, req, 503, json!({"ok": false, "error": e})),
    };
    let mut to_service = match conn.try_clone() {
        Ok(c) => c,
        Err(e) => return reply(app, req, 500, json!({"ok": false, "error": e.to_string()})),
    };
    if to_service.write_all(format!("{v}\n").as_bytes()).is_err() {
        return reply(app, req, 503, json!({"ok": false, "error": "ZO-AI's service closed the connection."}));
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
    while ok {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => ok = chunk(&mut out, &line),
        }
    }
    let _ = out.write_all(b"0\r\n\r\n");
    let _ = out.flush();
    // closing the connection tells the service to stop if the person went away
    if let Some((_, s)) = app.streams.lock().unwrap().remove(&id) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    drop(permit);
    drop(lease);
}

fn route(app: Arc<App>, mut req: Request) {
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    match (&method, path.as_str()) {
        (Method::Options, _) => return text(&app, req, 204, ""),
        (Method::Get, "/healthz") => return text(&app, req, 200, "ok"),
        (Method::Get, "/") | (Method::Get, "/index.html") => return html(&app, req, include_str!("../web/index.html").to_string(), pages::APP_CSP),
        (Method::Get, "/privacy") => return html(&app, req, pages::page("ZO-AI privacy notice", pages::PRIVACY_MD), pages::PLAIN_CSP),
        (Method::Get, "/terms") => return html(&app, req, pages::page("ZO-AI terms of use", pages::TERMS_MD), pages::PLAIN_CSP),
        _ => {}
    }
    let Some(r) = route_of(&method, &path) else { return text(&app, req, 404, "not found") };
    let who = match who(&app, &req) {
        Ok(w) => w,
        Err(f) => return fail(&app, req, f),
    };
    if !permits(&who.cred, r) {
        return fail(&app, req, Fail::new(403, "This device token may not do that."));
    }
    let lease = match app.tenants.acquire(&who.tenant, who.admin, Instant::now()) {
        Ok(l) => l,
        Err(e) => return fail(&app, req, e.into()),
    };
    let tenant = lease.tenant(!who.admin);
    let v = match body(&mut req, if r == Route::Sync { SYNC_BODY_LIMIT } else { BODY_LIMIT }) {
        Ok(v) => v,
        Err(f) => return fail(&app, req, f),
    };
    match r {
        Route::Send => send(&app, req, v, lease, tenant),
        Route::Sync => match sync_api::handle(&path, &v, &tenant) {
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
            let sent = match app.streams.lock().unwrap().get_mut(&id) {
                Some((t, s)) if *t == tenant.id => s.write_all(format!("{v}\n").as_bytes()).is_ok(),
                _ => false,
            };
            reply(&app, req, if sent { 200 } else { 404 }, json!({"ok": sent}))
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
            match op(&for_service(v, &tenant)) {
                Ok(r) => reply(&app, req, 200, r),
                Err(e) => reply(&app, req, 502, json!({"ok": false, "error": e})),
            }
        }
    }
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
    let no_auth = env("ZO_SERVER_NO_AUTH").as_deref() == Some("1");
    let account = env("FIREBASE_SERVICE_ACCOUNT");
    if !no_auth && account.is_none() {
        eprintln!("zo-server: set FIREBASE_SERVICE_ACCOUNT (and ZO_ADMIN_EMAILS), or ZO_SERVER_NO_AUTH=1 to try it on your own computer.");
        std::process::exit(2);
    }
    let policy = Policy::new(env("ZO_ADMIN_EMAILS"), env("ZO_BLOCKED_EMAILS"), env("ZO_MAX_USERS"));
    if !no_auth && policy.admin_count() == 0 {
        eprintln!("zo-server: ZO_ADMIN_EMAILS is empty: nobody is an admin; everybody is restricted.");
    }
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
    let secret = env("ZO_SERVER_SECRET");
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
    let app = Arc::new(App {
        no_auth,
        noauth_admin: no_auth && env("ZO_NOAUTH_ADMIN").as_deref() == Some("1"),
        verifier,
        policy,
        tenants,
        devices,
        streams: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        conns: AtomicUsize::new(0),
    });
    serve(app, server);
}

#[cfg(test)]
mod tests;
