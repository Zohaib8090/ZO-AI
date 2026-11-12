//! ZO-AI in the cloud. The same brain as the desktop app (zo-core's service), reachable over HTTPS behind a Firebase login, with
//! its chats, memory and settings kept in Firestore because the server's own disk does not last.
//!
//! Settings (environment):
//!   PORT                       where to listen (Render sets it)
//!   FIREBASE_SERVICE_ACCOUNT   the service account key file's contents (JSON): turns on the login check and Firestore
//!   ZO_ALLOWED_EMAILS          who may sign in, separated by commas (required with the login check)
//!   ZO_SERVER_SECRET           a long random text: API keys are stored in Firestore encrypted with it
//!   RENDER_EXTERNAL_URL        the server's own address (Render sets it): it calls itself every few minutes so it is not put to sleep
//!   ZO_ACCESS_TOKEN            the owner's own-devices key (at least 32 characters): `Authorization: Bearer <it>` is accepted on every /api call
//!                              (desktop sync, terminal client). Without FIREBASE_SERVICE_ACCOUNT the server runs "token-only": no Firebase
//!                              login, no Firestore, listens on 127.0.0.1 unless PORT and ZO_BIND (for example 0.0.0.0) are both set
//!   ZO_SERVER_NO_AUTH=1        for testing on your own computer only: no login, listens on 127.0.0.1 only, no Firestore

mod auth;
mod firestore;
mod sync;
mod sync_api;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};

struct App {
    project: String,
    allowed: Vec<String>,
    no_auth: bool,
    /// ZO_ACCESS_TOKEN (at least 32 characters), if set.
    access_token: Option<String>,
    /// Answers running right now: the web app can talk to one (approve a step, stop it) by its number.
    streams: Mutex<HashMap<u64, UnixStream>>,
    next: AtomicU64,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("header")
}

/// Set in the no-login test mode: nothing from another web address may talk to the server then (see `local_only`).
static NO_AUTH: AtomicBool = AtomicBool::new(false);

fn cors() -> Vec<Header> {
    if NO_AUTH.load(Ordering::Relaxed) {
        return Vec::new(); // no cross-site access: only the page the server itself serves
    }
    vec![
        header("Access-Control-Allow-Origin", "*"),
        header("Access-Control-Allow-Headers", "Authorization, Content-Type, X-ZO-Local"),
        header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        header("Access-Control-Max-Age", "86400"),
    ]
}

fn reply(req: Request, status: u16, body: Value) {
    let mut r = Response::from_string(body.to_string()).with_status_code(status).with_header(header("Content-Type", "application/json"));
    for h in cors() {
        r.add_header(h);
    }
    let _ = req.respond(r);
}

fn text(req: Request, status: u16, body: &str) {
    let mut r = Response::from_string(body).with_status_code(status);
    for h in cors() {
        r.add_header(h);
    }
    let _ = req.respond(r);
}

/// Same result for any two texts of any length, in time that does not depend on where they differ.
fn same_secret(a: &str, b: &str) -> bool {
    use sha2::{Digest, Sha256};
    let (x, y) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    x.iter().zip(y.iter()).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

/// The no-login test mode has no password, so it must only answer the page it serves itself, on this computer. A web page from
/// another site cannot add a custom header without the server's permission (which is never given), and a name that points at
/// this computer from elsewhere (DNS rebinding) has another Host, so both are refused.
fn local_only(req: &Request) -> Result<(), String> {
    fn get(req: &Request, name: &'static str) -> Option<String> {
        req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_string())
    }
    if get(req, "X-ZO-Local").as_deref() != Some("1") {
        return Err("The test mode only answers its own page.".into());
    }
    let host = get(req, "Host").unwrap_or_default().to_lowercase();
    if !host_is_local(&host) {
        return Err("The test mode only answers requests addressed to this computer.".into());
    }
    if let Some(origin) = get(req, "Origin") {
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

/// Who is asking, or why they are refused.
fn who(app: &App, req: &Request) -> Result<String, String> {
    if app.no_auth {
        return local_only(req).map(|_| "local".into());
    }
    let token = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string())
        .and_then(|v| v.strip_prefix("Bearer ").map(str::to_string))
        .ok_or("Sign in first.")?;
    if let Some(own) = &app.access_token {
        if same_secret(&token, own) {
            return Ok("access token".into());
        }
    }
    if app.project.is_empty() {
        return Err("The access token is not right.".into());
    }
    auth::verify(&token, &app.project, &app.allowed)
}

fn body(req: &mut Request) -> Result<Value, String> {
    let mut s = String::new();
    req.as_reader().take(20_000_000).read_to_string(&mut s).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&s).map_err(|_| "The request is not JSON.")?;
    if v.is_object() { Ok(v) } else { Err("The request must be a JSON object.".into()) }
}

fn connect() -> Result<UnixStream, String> {
    UnixStream::connect(zo_core::service::socket_path()).map_err(|e| format!("ZO-AI's service is not running: {e}"))
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

/// Sends a message and passes the answer on as it is written: one JSON object per line.
fn send(app: &Arc<App>, req: Request, mut v: Value) {
    v["op"] = json!("send");
    let conn = match connect() {
        Ok(c) => c,
        Err(e) => return reply(req, 503, json!({"ok": false, "error": e})),
    };
    let mut to_service = match conn.try_clone() {
        Ok(c) => c,
        Err(e) => return reply(req, 500, json!({"ok": false, "error": e.to_string()})),
    };
    if to_service.write_all(format!("{v}\n").as_bytes()).is_err() {
        return reply(req, 503, json!({"ok": false, "error": "ZO-AI's service closed the connection."}));
    }
    let id = app.next.fetch_add(1, Ordering::SeqCst);
    app.streams.lock().unwrap().insert(id, to_service);
    let mut out = req.into_writer();
    let cors = if NO_AUTH.load(Ordering::Relaxed) { "" } else { "Access-Control-Allow-Origin: *\r\n" };
    let head = &format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-cache\r\nX-Accel-Buffering: no\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n{cors}\r\n");
    let chunk = |out: &mut Box<dyn Write + Send>, line: &str| -> bool {
        out.write_all(format!("{:x}\r\n{line}\r\n", line.len()).as_bytes()).is_ok() && out.flush().is_ok()
    };
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
    if let Some(s) = app.streams.lock().unwrap().remove(&id) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
}

fn route(app: Arc<App>, mut req: Request) {
    let path = req.url().split('?').next().unwrap_or("").to_string();
    match (req.method().clone(), path.as_str()) {
        (Method::Options, _) => text(req, 204, ""),
        (Method::Get, "/healthz") => text(req, 200, "ok"),
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            let r = Response::from_string(include_str!("../web/index.html")).with_header(header("Content-Type", "text/html; charset=utf-8")).with_header(header("Cache-Control", "no-cache"));
            let _ = req.respond(r);
        }
        (Method::Post, "/api/op") | (Method::Post, "/api/send") | (Method::Post, "/api/reply") | (Method::Post, "/api/sync/manifest") | (Method::Post, "/api/sync/get") | (Method::Post, "/api/sync/put") | (Method::Post, "/api/sync/delete") => {
            if let Err(e) = who(&app, &req) {
                return reply(req, 401, json!({"ok": false, "error": e}));
            }
            let v = match body(&mut req) {
                Ok(v) => v,
                Err(e) => return reply(req, 400, json!({"ok": false, "error": e})),
            };
            match path.as_str() {
                "/api/send" => send(&app, req, v),
                p if p.starts_with("/api/sync/") => match sync_api::handle(p, &v) {
                    Ok(r) => reply(req, 200, r),
                    Err(e) => reply(req, 400, json!({"ok": false, "error": e})),
                },
                "/api/reply" => {
                    // a line for an answer that is running: {"stream": 3, "op": "stop"} or an approval
                    let id = v.get("stream").and_then(Value::as_u64).unwrap_or(0);
                    let sent = match app.streams.lock().unwrap().get_mut(&id) {
                        Some(s) => s.write_all(format!("{v}\n").as_bytes()).is_ok(),
                        None => false,
                    };
                    reply(req, if sent { 200 } else { 404 }, json!({"ok": sent}))
                }
                _ => {
                    if v.get("op").and_then(Value::as_str) == Some("send") {
                        return reply(req, 400, json!({"ok": false, "error": "Use /api/send to send a message."}));
                    }
                    match op(&v) {
                        Ok(r) => reply(req, 200, r),
                        Err(e) => reply(req, 502, json!({"ok": false, "error": e})),
                    }
                }
            }
        }
        _ => text(req, 404, "not found"),
    }
}

fn main() {
    let no_auth = env("ZO_SERVER_NO_AUTH").as_deref() == Some("1");
    let account = env("FIREBASE_SERVICE_ACCOUNT");
    let allowed: Vec<String> = env("ZO_ALLOWED_EMAILS").map(|s| s.split(',').map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()).collect()).unwrap_or_default();
    let access_token = env("ZO_ACCESS_TOKEN").filter(|t| {
        let long = t.chars().count() >= 32;
        if !long {
            eprintln!("zo-server: ZO_ACCESS_TOKEN is shorter than 32 characters, so it is ignored.");
        }
        long
    });
    // token-only: the access token is the only way in; no Firebase login and no Firestore
    let token_only = !no_auth && account.is_none() && access_token.is_some();
    if !no_auth && !token_only && (account.is_none() || allowed.is_empty()) {
        eprintln!("zo-server: set FIREBASE_SERVICE_ACCOUNT and ZO_ALLOWED_EMAILS (or ZO_ACCESS_TOKEN for token-only mode, or ZO_SERVER_NO_AUTH=1 to try it on your own computer).");
        std::process::exit(2);
    }
    let (config, data) = (zo_core::audit::config_dir(), zo_core::audit::data_dir());
    let _ = std::fs::create_dir_all(&config);
    let _ = std::fs::create_dir_all(&data);
    let keys_file = match env("ZO_AI_KEYS_FILE") {
        Some(p) => p.into(),
        None => {
            let p = config.join("keys.json");
            std::env::set_var("ZO_AI_KEYS_FILE", &p);
            p
        }
    };

    // Firestore first, so the service starts with the saved chats, memory and settings
    let mut project = env("FIREBASE_PROJECT_ID").unwrap_or_default();
    if let (Some(account), false) = (account, no_auth) {
        let fs = match firestore::Firestore::new(&account, &env("ZO_FIRESTORE_ROOT").unwrap_or_else(|| "zoai/owner".into())) {
            Ok(fs) => fs,
            Err(e) => {
                eprintln!("zo-server: {e}");
                std::process::exit(2);
            }
        };
        project = fs.project().to_string();
        let secret = env("ZO_SERVER_SECRET");
        if secret.is_none() {
            eprintln!("zo-server: ZO_SERVER_SECRET is not set: API keys entered here are lost when the server restarts.");
        }
        let mut sync = sync::Sync::new(fs, secret, keys_file);
        match sync.pull(&config, &data) {
            Ok((chats, notes)) => eprintln!("zo-server: fetched {chats} chats and {notes} memory notes from Firestore"),
            Err(e) => {
                // never start empty and then overwrite what is saved
                eprintln!("zo-server: cannot read Firestore, so nothing will be saved there: {e}");
                std::process::exit(3);
            }
        }
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(3));
            if let Err(e) = sync.push(&config, &data) {
                eprintln!("zo-server: {e}");
                std::thread::sleep(Duration::from_secs(20));
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
    let host = if no_auth {
        "127.0.0.1".to_string()
    } else if token_only {
        // token-only listens on this computer only, unless PORT and ZO_BIND are both set on purpose
        match (env("PORT"), env("ZO_BIND")) {
            (Some(_), Some(b)) => b,
            _ => "127.0.0.1".to_string(),
        }
    } else {
        "0.0.0.0".to_string()
    };
    let addr = format!("{host}:{port}");
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zo-server: cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("zo-server: listening on {addr}{}", if no_auth { " (no login: testing only)" } else if token_only { " (token-only: no Firebase login, no Firestore)" } else { "" });
    NO_AUTH.store(no_auth, Ordering::Relaxed);
    let app = Arc::new(App { project, allowed, no_auth, access_token, streams: Mutex::new(HashMap::new()), next: AtomicU64::new(1) });
    for req in server.incoming_requests() {
        let app = app.clone();
        std::thread::spawn(move || route(app, req));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_computer_counts_as_local() {
        for h in ["127.0.0.1", "127.0.0.1:8787", "localhost:8787", "[::1]:8787", "[::1]"] {
            assert!(host_is_local(h), "{h}");
        }
        for h in ["", "evil.example", "127.0.0.1.evil.example", "localhost.evil.example:80", "[::1]x", "0.0.0.0:8787", "attacker.test:8787"] {
            assert!(!host_is_local(h), "{h}");
        }
    }
}
