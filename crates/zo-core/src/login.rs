//! Signing in to the Zohara Agent server, for the desktop app and the terminal client (no GUI code here). See docs/SYNC.md.
//!
//! Nothing here runs unless the person chooses to sign in: the app works fully without it, and nothing is sent anywhere before.
//! Two ways in, both ending in a **device credential** (`zd_...`, issued by the server, stored hashed there, revocable):
//!
//! * **E-mail and password**: `sign_in_password` / `sign_up` (Firebase REST) give an ID token, and `exchange` trades it for the
//!   credential. The e-mail address must be verified (`send_verification`).
//! * **Browser** (also how Google sign-in works): `run_device_flow` asks the server for a short code, the person types it on the
//!   server's approval page after signing in there, and the server hands the credential over once.
//!
//! The credential is kept in the key store (id `sync-token`, the system keyring); the account's e-mail and the on/off switch are
//! in ~/.config/zohara-agent/sync.json, never the credential. It is never logged, and never put in an error text.
//! Every call that goes out takes its base address as a parameter (`*_at`), so tests can talk to a mock on this computer.

use crate::sync;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The Firebase web API key of project zo-ai-144. It is public (it is in the web app too); it only names the project.
pub const FIREBASE_API_KEY: &str = "AIzaSyBU_nnDL5JKXwWHNTRMKYK2RCvNGXv4PaQ";
pub const FIREBASE_BASE: &str = "https://identitytoolkit.googleapis.com/v1";
pub const DEFAULT_SERVER_URL: &str = "https://zo-ai-server.onrender.com";
/// The key store id of the credential (the same id sync.rs always used for its token).
const CREDENTIAL_ID: &str = "sync-token";

// ---------------------------------------------------------------------------------------------------------------------------
// the server address

/// Where the Zohara Agent server is: the environment `ZO_SERVER_URL`, else the address saved in sync.json, else the default.
/// Every one of them has to pass `sync::clean_url` (https; plain http only for this computer), because a credential goes there.
pub fn server_url() -> Result<String, String> {
    resolve_server_url(std::env::var("ZO_SERVER_URL").ok().as_deref(), &sync::config().url)
}

/// The pure part of `server_url`.
pub fn resolve_server_url(env: Option<&str>, saved: &str) -> Result<String, String> {
    for (what, v) in [("ZO_SERVER_URL", env.unwrap_or("")), ("the saved server address", saved)] {
        if !v.trim().is_empty() {
            return sync::clean_url(v).map_err(|e| format!("{what}: {e}"));
        }
    }
    Ok(DEFAULT_SERVER_URL.to_string())
}

// ---------------------------------------------------------------------------------------------------------------------------
// what a person sees when something fails

/// Firebase's error code (`EMAIL_EXISTS`, `WEAK_PASSWORD : Password should be...`, ...) as a friendly sentence.
pub fn friendly_firebase_error(message: &str) -> String {
    let code = message.split([' ', ':']).next().unwrap_or("").trim();
    match code {
        "EMAIL_NOT_FOUND" | "INVALID_PASSWORD" | "INVALID_LOGIN_CREDENTIALS" => "Wrong e-mail or password.".into(),
        "EMAIL_EXISTS" => "There is already an account with this e-mail address. Sign in instead, or reset the password.".into(),
        "WEAK_PASSWORD" => "That password is too weak: use at least 6 characters.".into(),
        "TOO_MANY_ATTEMPTS_TRY_LATER" => "Too many attempts. Wait a few minutes and try again.".into(),
        "USER_DISABLED" => "This account has been disabled.".into(),
        "INVALID_EMAIL" => "That does not look like an e-mail address.".into(),
        "MISSING_EMAIL" => "Type your e-mail address.".into(),
        "MISSING_PASSWORD" => "Type your password.".into(),
        "OPERATION_NOT_ALLOWED" | "PASSWORD_LOGIN_DISABLED" => "Signing in with e-mail and password is not switched on for this project.".into(),
        "INVALID_ID_TOKEN" | "CREDENTIAL_TOO_OLD_LOGIN_AGAIN" | "TOKEN_EXPIRED" | "USER_NOT_FOUND" => "That sign-in has expired. Sign in again.".into(),
        "QUOTA_EXCEEDED" => "The sign-in service is busy. Try again later.".into(),
        "API_KEY_INVALID" | "API_KEY_NOT_VALID." => "The sign-in service did not accept this app's key.".into(),
        c if !c.is_empty() && c.len() < 60 && c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_' || ch.is_ascii_digit()) => format!("Sign-in failed ({c})."),
        _ => "Sign-in failed.".into(),
    }
}

fn check_email(email: &str) -> Result<String, String> {
    let e = email.trim();
    let ok = e.len() <= 254 && !e.chars().any(|c| c.is_whitespace() || c.is_control()) && matches!(e.split_once('@'), Some((a, b)) if !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.'));
    if ok {
        Ok(e.to_string())
    } else {
        Err(friendly_firebase_error("INVALID_EMAIL"))
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// HTTP

/// (status, JSON body or Null). Err = the other side could not be reached.
fn http_post(url: &str, query: &[(&str, &str)], bearer: Option<&str>, body: &Value) -> Result<(u16, Value), String> {
    let mut req = ureq::post(url).timeout(Duration::from_secs(20));
    for (k, v) in query {
        req = req.query(k, v);
    }
    if let Some(b) = bearer {
        req = req.set("Authorization", &format!("Bearer {b}"));
    }
    match req.send_json(body.clone()) {
        Ok(r) => {
            let code = r.status();
            Ok((code, r.into_json::<Value>().unwrap_or(Value::Null)))
        }
        Err(ureq::Error::Status(code, r)) => Ok((code, r.into_json::<Value>().unwrap_or(Value::Null))),
        // the kind only: the text of a transport error carries the address
        Err(ureq::Error::Transport(t)) => Err(format!("Cannot reach the server: {}", t.kind())),
    }
}

fn fb_call(base: &str, endpoint: &str, body: &Value) -> Result<Value, String> {
    let base = sync::clean_url(base)?;
    let (code, v) = http_post(&format!("{base}/{endpoint}"), &[("key", FIREBASE_API_KEY)], None, body).map_err(|e| e.replace("the server", "the sign-in service"))?;
    if code == 200 {
        return Ok(v);
    }
    let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
    if msg.is_empty() && code >= 500 {
        return Err("The sign-in service is not answering. Try again later.".into());
    }
    Err(friendly_firebase_error(msg))
}

/// Why a call to the Zohara Agent server failed: a hiccup worth trying again, or an answer.
#[derive(Debug)]
enum Fail {
    Transient(String),
    Final(String),
}

impl Fail {
    fn text(self) -> String {
        match self {
            Fail::Transient(s) | Fail::Final(s) => s,
        }
    }
}

fn srv_call(base: &str, path: &str, bearer: Option<&str>, body: &Value) -> Result<Value, Fail> {
    let base = sync::clean_url(base).map_err(Fail::Final)?;
    let (code, v) = http_post(&format!("{base}{path}"), &[], bearer, body).map_err(Fail::Transient)?;
    let err = v.get("error").and_then(Value::as_str).filter(|e| !e.is_empty() && e.len() < 400).map(str::to_string);
    match code {
        200..=299 if v.get("ok").and_then(Value::as_bool) != Some(false) => Ok(v),
        200..=299 => Err(Fail::Final(err.unwrap_or_else(|| "The server refused.".into()))),
        401 => Err(Fail::Final(err.unwrap_or_else(|| "The sign-in was not accepted. Sign in again.".into()))),
        429 => Err(Fail::Transient("Too many requests. Wait a minute and try again.".into())),
        500..=599 => Err(Fail::Transient(format!("The server answered {code}."))),
        _ => Err(Fail::Final(err.unwrap_or_else(|| format!("The server answered {code}.")))),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// e-mail and password (Firebase REST)

/// A Firebase sign-in. The ID token is short-lived and only for `exchange` / `send_verification`; it is not kept.
#[derive(Clone)]
pub struct Session {
    pub id_token: String,
    pub email: String,
    /// `Some(false)` = the address is not verified yet (the server will not accept it); `None` = not known.
    pub email_verified: Option<bool>,
    /// Only for `sign_up`: the verification e-mail went out.
    pub verification_sent: bool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session {{ email: {:?}, email_verified: {:?}, id_token: <hidden> }}", self.email, self.email_verified)
    }
}

fn session_from(v: &Value, fallback_email: &str) -> Result<Session, String> {
    let id_token = v.get("idToken").and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or("The sign-in service's answer was not understood.")?;
    let email = v.get("email").and_then(Value::as_str).unwrap_or(fallback_email);
    Ok(Session { id_token: id_token.to_string(), email: email.to_string(), email_verified: None, verification_sent: false })
}

pub fn sign_in_password(email: &str, password: &str) -> Result<Session, String> {
    sign_in_password_at(FIREBASE_BASE, email, password)
}

pub fn sign_in_password_at(base: &str, email: &str, password: &str) -> Result<Session, String> {
    let email = check_email(email)?;
    if password.is_empty() {
        return Err(friendly_firebase_error("MISSING_PASSWORD"));
    }
    let v = fb_call(base, "accounts:signInWithPassword", &json!({"email": email, "password": password, "returnSecureToken": true}))?;
    let mut s = session_from(&v, &email)?;
    // whether the address is verified (best effort: the server checks it anyway)
    s.email_verified = fb_call(base, "accounts:lookup", &json!({"idToken": s.id_token})).ok().and_then(|l| l.pointer("/users/0/emailVerified").and_then(Value::as_bool));
    Ok(s)
}

/// Creates the account and sends the verification e-mail. The account is usable once the person has clicked the link in it.
pub fn sign_up(email: &str, password: &str) -> Result<Session, String> {
    sign_up_at(FIREBASE_BASE, email, password)
}

pub fn sign_up_at(base: &str, email: &str, password: &str) -> Result<Session, String> {
    let email = check_email(email)?;
    if password.chars().count() < 6 {
        return Err(friendly_firebase_error("WEAK_PASSWORD"));
    }
    let v = fb_call(base, "accounts:signUp", &json!({"email": email, "password": password, "returnSecureToken": true}))?;
    let mut s = session_from(&v, &email)?;
    s.email_verified = Some(false);
    s.verification_sent = send_verification_at(base, &s.id_token).is_ok();
    Ok(s)
}

pub fn send_verification(id_token: &str) -> Result<(), String> {
    send_verification_at(FIREBASE_BASE, id_token)
}

pub fn send_verification_at(base: &str, id_token: &str) -> Result<(), String> {
    fb_call(base, "accounts:sendOobCode", &json!({"requestType": "VERIFY_EMAIL", "idToken": id_token})).map(|_| ())
}

pub fn reset_password(email: &str) -> Result<(), String> {
    reset_password_at(FIREBASE_BASE, email)
}

pub fn reset_password_at(base: &str, email: &str) -> Result<(), String> {
    let email = check_email(email)?;
    match fb_call(base, "accounts:sendOobCode", &json!({"requestType": "PASSWORD_RESET", "email": email})) {
        // do not tell whether an address has an account
        Err(e) if e == friendly_firebase_error("EMAIL_NOT_FOUND") => Ok(()),
        r => r.map(|_| ()),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// the server

/// A signed-in device: the credential and the account it belongs to. Never printed (see `Debug`).
#[derive(Clone)]
pub struct Login {
    pub credential: String,
    pub email: String,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Login {{ email: {:?}, credential: <hidden> }}", self.email)
    }
}

fn check_kind(kind: &str) -> Result<(), String> {
    if matches!(kind, "desktop" | "cli") {
        Ok(())
    } else {
        Err("The kind of device must be desktop or cli.".into())
    }
}

/// A name for this device as the person will see it in "signed-in devices" (1..60 characters).
pub fn clean_device_name(name: &str) -> Result<String, String> {
    let n: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(60).collect();
    if n.trim().is_empty() {
        Err("The device needs a name.".into())
    } else {
        Ok(n.trim().to_string())
    }
}

/// "Zohara Agent desktop on <host name of this computer>".
pub fn default_device_name(kind: &str) -> String {
    let host = std::env::var("HOSTNAME").ok().filter(|h| !h.trim().is_empty()).or_else(|| std::fs::read_to_string("/etc/hostname").ok()).unwrap_or_default();
    let host = host.trim();
    let what = if kind == "cli" { "terminal" } else { "desktop" };
    let n = if host.is_empty() { format!("Zohara Agent {what}") } else { format!("Zohara Agent {what} on {host}") };
    clean_device_name(&n).unwrap_or_else(|_| "Zohara Agent".into())
}

fn credential_from(v: &Value) -> Result<Login, String> {
    let c = v.get("credential").and_then(Value::as_str).unwrap_or("");
    if !c.starts_with("zd_") || c.len() < 20 || c.len() > 200 || c.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err("The server's answer was not understood.".into());
    }
    Ok(Login { credential: c.to_string(), email: v.get("email").and_then(Value::as_str).unwrap_or("").to_string() })
}

/// Trades a Firebase ID token (from `sign_in_password`) for this device's credential.
pub fn exchange(id_token: &str, name: &str, kind: &str) -> Result<Login, String> {
    exchange_at(&server_url()?, id_token, name, kind)
}

pub fn exchange_at(base: &str, id_token: &str, name: &str, kind: &str) -> Result<Login, String> {
    check_kind(kind)?;
    let name = clean_device_name(name)?;
    let v = srv_call(base, "/api/auth/exchange", Some(id_token), &json!({"name": name, "kind": kind})).map_err(|f| {
        let t = f.text();
        // terms can only be accepted in the web app: say where, so the person is not left at a dead end
        if t.to_lowercase().contains("the terms") {
            format!("The terms have not been accepted yet. Open {} in a browser, sign in there once and accept the privacy notice and the terms, then sign in here again.", base.trim_end_matches('/'))
        } else {
            t
        }
    })?;
    credential_from(&v)
}

/// The server's answer to `device_start`: show `user_code` to the person and open `verify_url`.
#[derive(Clone, Debug)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verify_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub fn device_start_at(base: &str, name: &str, kind: &str) -> Result<DeviceCode, String> {
    check_kind(kind)?;
    let name = clean_device_name(name)?;
    let v = srv_call(base, "/api/auth/device_start", None, &json!({"name": name, "kind": kind})).map_err(Fail::text)?;
    parse_device_code(&v)
}

pub fn parse_device_code(v: &Value) -> Result<DeviceCode, String> {
    let bad = || "The server's answer was not understood.".to_string();
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let (device_code, user_code) = (s("device_code"), s("user_code"));
    if device_code.len() < 20 || device_code.len() > 200 || user_code.is_empty() || user_code.len() > 20 || user_code.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '-')) {
        return Err(bad());
    }
    // the person is sent to this address: it has to be https (or this computer)
    let verify_url = sync::clean_url(&s("verify_url")).map_err(|_| bad())?;
    Ok(DeviceCode {
        device_code,
        user_code,
        verify_url,
        expires_in: v.get("expires_in").and_then(Value::as_u64).unwrap_or(600).clamp(30, 3600),
        interval: v.get("interval").and_then(Value::as_u64).unwrap_or(3).clamp(1, 30),
    })
}

/// What one `device_poll` answer means.
#[derive(Clone, Debug, PartialEq)]
pub enum Poll {
    Pending,
    SlowDown,
    Approved(Login),
    Denied,
    Expired,
}

impl PartialEq for Login {
    fn eq(&self, o: &Login) -> bool {
        self.credential == o.credential && self.email == o.email
    }
}

pub fn parse_poll(v: &Value) -> Result<Poll, String> {
    match v.get("status").and_then(Value::as_str) {
        Some("pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown),
        Some("approved") => credential_from(v).map(Poll::Approved),
        Some("denied") => Ok(Poll::Denied),
        Some("expired") => Ok(Poll::Expired),
        _ => Err("The server's answer was not understood.".into()),
    }
}

/// What to do after a poll answer.
#[derive(Debug, PartialEq)]
pub enum Next {
    /// Poll again after this many seconds.
    Wait(u64),
    Done(Login),
    Stop(String),
}

/// The polling pace: the server's interval, made longer when it says `slow_down`, and the time the code is good for.
#[derive(Debug)]
pub struct Pacer {
    interval: u64,
    waited: u64,
    expires_in: u64,
}

impl Pacer {
    pub fn new(interval: u64, expires_in: u64) -> Pacer {
        Pacer { interval: interval.clamp(1, 30), waited: 0, expires_in }
    }

    pub fn interval(&self) -> u64 {
        self.interval
    }

    pub fn on(&mut self, p: Poll) -> Next {
        match p {
            Poll::Approved(l) => Next::Done(l),
            Poll::Denied => Next::Stop("The sign-in was denied on the approval page.".into()),
            Poll::Expired => Next::Stop("The code expired. Start again.".into()),
            Poll::Pending | Poll::SlowDown => {
                if p == Poll::SlowDown {
                    self.interval = (self.interval + 2).min(30);
                }
                if self.waited + self.interval > self.expires_in + 10 {
                    return Next::Stop("The code expired. Start again.".into());
                }
                self.waited += self.interval;
                Next::Wait(self.interval)
            }
        }
    }
}

/// One poll. A hiccup (network, 5xx, 429) is an `Err(Transient)`; the loop tolerates a few in a row.
fn device_poll(base: &str, device_code: &str) -> Result<Poll, Fail> {
    let v = srv_call(base, "/api/auth/device_poll", None, &json!({"device_code": device_code}))?;
    parse_poll(&v).map_err(Fail::Final)
}

/// Signs in through the browser: starts the flow, tells the caller the code (`on_code(user_code, verify_url)`, once), and waits
/// until the person has approved it on the web page. Blocks (run it in a thread); set `cancel` to stop it.
pub fn run_device_flow(name: &str, kind: &str, on_code: impl Fn(&str, &str), cancel: &AtomicBool) -> Result<Login, String> {
    run_device_flow_at(&server_url()?, name, kind, on_code, cancel, &sleep_cancellable)
}

/// Waits `secs` seconds, looking at `cancel` every 200 ms.
fn sleep_cancellable(secs: u64, cancel: &AtomicBool) {
    for _ in 0..secs * 5 {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn run_device_flow_at(base: &str, name: &str, kind: &str, on_code: impl Fn(&str, &str), cancel: &AtomicBool, sleep: &dyn Fn(u64, &AtomicBool)) -> Result<Login, String> {
    const CANCELLED: &str = "Cancelled.";
    if cancel.load(Ordering::SeqCst) {
        return Err(CANCELLED.into());
    }
    let code = device_start_at(base, name, kind)?;
    on_code(&code.user_code, &code.verify_url);
    let mut pacer = Pacer::new(code.interval, code.expires_in);
    let mut hiccups = 0;
    loop {
        sleep(pacer.interval(), cancel);
        if cancel.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        match device_poll(base, &code.device_code) {
            Ok(p) => {
                hiccups = 0;
                match pacer.on(p) {
                    Next::Done(l) => return Ok(l),
                    Next::Stop(e) => return Err(e),
                    Next::Wait(_) => {}
                }
            }
            Err(Fail::Transient(e)) => {
                hiccups += 1;
                if hiccups >= 6 {
                    return Err(e);
                }
                // the time still runs
                if let Next::Stop(e) = pacer.on(Poll::Pending) {
                    return Err(e);
                }
            }
            Err(Fail::Final(e)) => return Err(e),
        }
    }
}

/// Tells the server to revoke this device's credential.
pub fn signout_at(base: &str, credential: &str) -> Result<(), String> {
    let base = sync::clean_url(base)?;
    match http_post(&format!("{base}/api/auth/signout"), &[], Some(credential), &json!({}))? {
        // 401: already revoked (for instance in the web app), nothing is left to revoke
        (200..=299 | 401, _) => Ok(()),
        (429, _) => Err("Too many requests. Wait a minute and try again.".into()),
        (c, _) => Err(format!("The server answered {c}.")),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// what this computer remembers

/// Which program on this computer a sign-in belongs to. The desktop app (credential scope `sync`) and the terminal client `zo`
/// (scope `full`) each have their OWN credential, so signing out of one never signs the other out, and the app's narrower
/// credential is never used where a full one is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    Desktop,
    Cli,
}

impl Client {
    fn credential_id(self) -> &'static str {
        match self {
            Client::Desktop => CREDENTIAL_ID,
            Client::Cli => "cli-token",
        }
    }
}

fn cli_record_path() -> std::path::PathBuf {
    crate::audit::config_dir().join("cli-login.json")
}

/// The e-mail of a client's sign-in, from files only (no key store, so it never waits on the keyring).
pub fn email_of(client: Client) -> String {
    match client {
        Client::Desktop => sync::config().email,
        Client::Cli => std::fs::read_to_string(cli_record_path()).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| v.get("email").and_then(Value::as_str).map(str::to_string)).unwrap_or_default(),
    }
}

/// The credential of a client (key store, or the private file when there is no key store).
pub fn credential_of(client: Client) -> Option<String> {
    sync::load_credential(client.credential_id())
}

/// Quick check for the screen (only reads small files: never waits for the key store, so it is safe on the window's thread).
/// A sign-in whose credential was lost or revoked is found out by the next sync round, which clears it.
pub fn signed_in_quick() -> Option<String> {
    let e = email_of(Client::Desktop);
    (!e.is_empty()).then_some(e)
}

/// The desktop app's signed-in account, or None (also checks that the credential is there).
pub fn signed_in() -> Option<String> {
    signed_in_as(Client::Desktop)
}

pub fn signed_in_as(client: Client) -> Option<String> {
    let email = email_of(client);
    (!email.is_empty() && credential_of(client).is_some()).then_some(email)
}

/// Remembers the desktop app's sign-in: the credential goes to the key store, the e-mail to sync.json, syncing is switched on and
/// the background loop is started (a round runs within a second). An address chosen by hand earlier is kept.
pub fn save_login(login: &Login) -> Result<(), String> {
    save_login_as(Client::Desktop, login)
}

pub fn save_login_as(client: Client, login: &Login) -> Result<(), String> {
    let cred = login.credential.trim();
    if !cred.starts_with("zd_") {
        return Err("That is not a device credential.".into());
    }
    let id = client.credential_id();
    if let Err(e) = sync::store_credential(id, cred) {
        // the server already issued it: do not leave an orphan device behind
        if let Ok(base) = server_url() {
            let _ = signout_at(&base, cred);
        }
        return Err(format!("The sign-in could not be stored safely: {e}"));
    }
    match client {
        Client::Desktop => {
            let cfg = sync::config();
            if let Err(e) = sync::write_config(&cfg.url, true, &login.email) {
                sync::forget_credential(id);
                if let Ok(base) = server_url() {
                    let _ = signout_at(&base, cred);
                }
                return Err(format!("The sign-in could not be saved: {e}"));
            }
            // (not in tests: a background loop would keep running in the other tests' environments)
            #[cfg(not(test))]
            sync::start();
            sync::kick();
        }
        Client::Cli => {
            let p = cli_record_path();
            let r = p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&p, json!({"email": login.email}).to_string()));
            if let Err(e) = r {
                sync::forget_credential(id);
                if let Ok(base) = server_url() {
                    let _ = signout_at(&base, cred);
                }
                return Err(format!("The sign-in could not be saved: {e}"));
            }
        }
    }
    Ok(())
}

/// Forgets the desktop sign-in on this computer: the credential, the e-mail, the switch and the sync status. Chats, memory and
/// personalities stay as they are. (This does not tell the server: see `sign_out`.)
pub fn clear_login() {
    clear_login_as(Client::Desktop)
}

pub fn clear_login_as(client: Client) {
    sync::forget_credential(client.credential_id());
    match client {
        Client::Desktop => {
            let cfg = sync::config();
            let _ = sync::write_config(&cfg.url, false, "");
            sync::reset_status();
        }
        Client::Cli => {
            let _ = std::fs::remove_file(cli_record_path());
        }
    }
}

/// Signs the desktop app out: the server is told to revoke this device's credential, and the sign-in is forgotten here either
/// way. An `Err` says that the server could not be told (the device can then be removed in the web app's "signed-in devices").
pub fn sign_out() -> Result<(), String> {
    sign_out_as(Client::Desktop)
}

pub fn sign_out_as(client: Client) -> Result<(), String> {
    let cred = credential_of(client);
    let told = match (cred, server_url()) {
        (Some(c), Ok(base)) => signout_at(&base, &c),
        (Some(_), Err(e)) => Err(e),
        (None, _) => Ok(()),
    };
    clear_login_as(client);
    told.map_err(|e| format!("Signed out here, but the server could not be told ({e}). Remove this device in the web app under signed-in devices."))
}

/// A device set up before sign-in existed (a pasted token, syncing on, no account e-mail) is not "signed in": nothing may be sent
/// for it. Its credential is forgotten (and the server told, best effort) and the switch turned off. Returns true when it did.
pub fn drop_legacy_setup() -> bool {
    let c = sync::config();
    if !c.enabled || !c.email.is_empty() {
        return false;
    }
    let _ = sign_out_as(Client::Desktop);
    true
}

// ---------------------------------------------------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TEST_ENV_LOCK;
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    // ---- pure parts -------------------------------------------------------------------------------------------------------

    #[test]
    fn firebase_errors_are_friendly() {
        for c in ["EMAIL_NOT_FOUND", "INVALID_PASSWORD", "INVALID_LOGIN_CREDENTIALS"] {
            assert_eq!(friendly_firebase_error(c), "Wrong e-mail or password.");
        }
        assert!(friendly_firebase_error("EMAIL_EXISTS").contains("already an account"));
        let weak = friendly_firebase_error("WEAK_PASSWORD : Password should be at least 6 characters");
        assert!(weak.contains("at least 6"), "{weak}");
        assert!(friendly_firebase_error("TOO_MANY_ATTEMPTS_TRY_LATER").contains("Too many attempts"));
        assert!(friendly_firebase_error("USER_DISABLED").contains("disabled"));
        assert!(friendly_firebase_error("INVALID_EMAIL").contains("e-mail"));
        assert_eq!(friendly_firebase_error("SOME_NEW_CODE"), "Sign-in failed (SOME_NEW_CODE).");
        assert_eq!(friendly_firebase_error(""), "Sign-in failed.");
        assert_eq!(friendly_firebase_error("<html>junk"), "Sign-in failed.");
    }

    #[test]
    fn server_url_order_and_rules() {
        assert_eq!(resolve_server_url(None, "").unwrap(), DEFAULT_SERVER_URL);
        assert_eq!(resolve_server_url(None, "https://saved.example/").unwrap(), "https://saved.example");
        assert_eq!(resolve_server_url(Some("https://env.example"), "https://saved.example").unwrap(), "https://env.example");
        assert_eq!(resolve_server_url(Some("  "), "https://saved.example").unwrap(), "https://saved.example");
        assert_eq!(resolve_server_url(Some("http://127.0.0.1:8799"), "").unwrap(), "http://127.0.0.1:8799");
        // http is only for this computer; tricks are refused (an error, not a silent fall back to the default)
        assert!(resolve_server_url(Some("http://example.com"), "").is_err());
        assert!(resolve_server_url(Some("http://localhost:80@evil.example"), "").is_err());
        assert!(resolve_server_url(None, "ftp://x").is_err());
        assert!(resolve_server_url(Some("https://a b"), "").is_err());
    }

    #[test]
    fn email_and_names_are_checked() {
        assert!(check_email(" a@b.co ").is_ok());
        for bad in ["", "abc", "@b.co", "a@b", "a b@c.de", "a@.com"] {
            assert!(check_email(bad).is_err(), "{bad}");
        }
        assert!(clean_device_name("  ").is_err());
        assert_eq!(clean_device_name(&"x".repeat(100)).unwrap().len(), 60);
        assert!(default_device_name("cli").starts_with("Zohara Agent terminal"));
        assert!(check_kind("phone").is_err());
    }

    fn login() -> Login {
        Login { credential: format!("zd_{}", "A".repeat(43)), email: "a@b.co".into() }
    }

    #[test]
    fn debug_never_shows_the_credential() {
        let l = login();
        assert!(!format!("{l:?}").contains("AAAA"));
        let s = Session { id_token: "SECRET-TOKEN".into(), email: "a@b.co".into(), email_verified: None, verification_sent: false };
        assert!(!format!("{s:?}").contains("SECRET"));
    }

    #[test]
    fn poll_answers_are_parsed() {
        assert_eq!(parse_poll(&json!({"status": "pending"})).unwrap(), Poll::Pending);
        assert_eq!(parse_poll(&json!({"status": "slow_down"})).unwrap(), Poll::SlowDown);
        assert_eq!(parse_poll(&json!({"status": "denied"})).unwrap(), Poll::Denied);
        assert_eq!(parse_poll(&json!({"status": "expired"})).unwrap(), Poll::Expired);
        let l = login();
        assert_eq!(parse_poll(&json!({"status": "approved", "credential": l.credential, "email": l.email})).unwrap(), Poll::Approved(l));
        assert!(parse_poll(&json!({"status": "approved", "credential": "nope"})).is_err());
        assert!(parse_poll(&json!({"status": "weird"})).is_err());
        assert!(parse_poll(&json!({})).is_err());
    }

    #[test]
    fn device_code_answer_is_checked() {
        let ok = json!({"device_code": "d".repeat(43), "user_code": "ABCD-EFGH", "verify_url": "https://x.example/device", "expires_in": 600, "interval": 3});
        let c = parse_device_code(&ok).unwrap();
        assert_eq!((c.user_code.as_str(), c.interval, c.expires_in), ("ABCD-EFGH", 3, 600));
        let mut bad = ok.clone();
        bad["verify_url"] = json!("http://evil.example/device");
        assert!(parse_device_code(&bad).is_err());
        bad = ok.clone();
        bad["user_code"] = json!("AB CD");
        assert!(parse_device_code(&bad).is_err());
        bad = ok.clone();
        bad["device_code"] = json!("short");
        assert!(parse_device_code(&bad).is_err());
    }

    #[test]
    fn pacer_pending_slow_down_approved() {
        let mut p = Pacer::new(3, 600);
        assert_eq!(p.on(Poll::Pending), Next::Wait(3));
        assert_eq!(p.on(Poll::SlowDown), Next::Wait(5));
        assert_eq!(p.on(Poll::Pending), Next::Wait(5));
        assert_eq!(p.on(Poll::Approved(login())), Next::Done(login()));
        // the pace is capped
        let mut p = Pacer::new(29, 6000);
        p.on(Poll::SlowDown);
        assert_eq!(p.on(Poll::SlowDown), Next::Wait(30));
    }

    #[test]
    fn pacer_denied_expired_and_timeout() {
        let mut p = Pacer::new(3, 600);
        assert!(matches!(p.on(Poll::Denied), Next::Stop(e) if e.contains("denied")));
        assert!(matches!(p.on(Poll::Expired), Next::Stop(e) if e.contains("expired")));
        // the code's own time runs out even if the server keeps saying pending
        let mut p = Pacer::new(3, 30);
        let mut n = 0;
        let r = loop {
            n += 1;
            match p.on(Poll::Pending) {
                Next::Wait(_) if n < 100 => {}
                other => break other,
            }
        };
        assert!(matches!(r, Next::Stop(e) if e.contains("expired")));
        assert!(n <= 15, "{n}");
    }

    // ---- a tiny mock of Firebase REST and the Zohara Agent server ----------------------------------------------------------------

    #[derive(Default)]
    struct Mock {
        /// e-mail -> (password, verified)
        users: BTreeMap<String, (String, bool)>,
        /// the credentials that are valid
        creds: Vec<String>,
        /// the scripted answers of device_poll, in order; the last one repeats
        script: Vec<Value>,
        polls: usize,
        start_calls: usize,
        saw_bearer_on_start: bool,
        verification_mails: usize,
        /// the one credential handed out (its record is deleted when it is polled)
        pending_credential: Option<String>,
        log: Vec<String>,
    }

    fn read_request(s: &mut std::net::TcpStream) -> Option<(String, String, String, Value)> {
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
        let first = head.lines().next()?.to_string();
        let mut it = first.split(' ');
        it.next();
        let path = it.next()?.to_string();
        let bearer = head.lines().find_map(|l| l.strip_prefix("authorization: Bearer ").or_else(|| l.strip_prefix("Authorization: Bearer "))).unwrap_or("").to_string();
        Some((path.clone(), path, bearer, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }

    fn reply(s: &mut std::net::TcpStream, code: u16, v: &Value) {
        let b = v.to_string();
        let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
    }

    fn fb_err(m: &str) -> (u16, Value) {
        (400, json!({"error": {"code": 400, "message": m}}))
    }

    fn handle(st: &Mutex<Mock>, full: &str, bearer: &str, body: &Value) -> (u16, Value) {
        let path = full.split('?').next().unwrap_or("");
        let mut m = st.lock().unwrap();
        m.log.push(format!("{path} key={}", full.contains(&format!("key={FIREBASE_API_KEY}"))));
        let email = body.get("email").and_then(Value::as_str).unwrap_or("").to_string();
        let tok = |e: &str| format!("idtok:{e}");
        match path {
            "/fb/accounts:signUp" => {
                if m.users.contains_key(&email) {
                    return fb_err("EMAIL_EXISTS");
                }
                let pw = body["password"].as_str().unwrap_or("").to_string();
                if pw.len() < 6 {
                    return fb_err("WEAK_PASSWORD : Password should be at least 6 characters");
                }
                m.users.insert(email.clone(), (pw, false));
                (200, json!({"idToken": tok(&email), "email": email}))
            }
            "/fb/accounts:signInWithPassword" => match m.users.get(&email) {
                Some((pw, _)) if Some(pw.as_str()) == body["password"].as_str() => (200, json!({"idToken": tok(&email), "email": email})),
                _ => fb_err("INVALID_LOGIN_CREDENTIALS"),
            },
            "/fb/accounts:lookup" => {
                let e = body["idToken"].as_str().unwrap_or("").trim_start_matches("idtok:").to_string();
                match m.users.get(&e) {
                    Some((_, v)) => (200, json!({"users": [{"email": e, "emailVerified": v}]})),
                    None => fb_err("INVALID_ID_TOKEN"),
                }
            }
            "/fb/accounts:sendOobCode" => match body["requestType"].as_str() {
                Some("VERIFY_EMAIL") => {
                    m.verification_mails += 1;
                    (200, json!({"email": "x"}))
                }
                Some("PASSWORD_RESET") if m.users.contains_key(&email) => (200, json!({"email": email})),
                _ => fb_err("EMAIL_NOT_FOUND"),
            },
            "/api/auth/exchange" => {
                let e = bearer.trim_start_matches("idtok:").to_string();
                match m.users.get(&e) {
                    None => (401, json!({"ok": false, "error": "Sign in again."})),
                    Some((_, false)) => (403, json!({"ok": false, "error": "Verify your e-mail address first."})),
                    Some(_) => {
                        let c = format!("zd_exchange_{:0>40}", m.creds.len());
                        m.creds.push(c.clone());
                        (200, json!({"ok": true, "credential": c, "email": e}))
                    }
                }
            }
            "/api/auth/device_start" => {
                m.start_calls += 1;
                m.saw_bearer_on_start |= !bearer.is_empty();
                if body["kind"].as_str().is_none() || body["name"].as_str().is_none() {
                    return (400, json!({"ok": false, "error": "bad body"}));
                }
                (200, json!({"ok": true, "device_code": "D".repeat(43), "user_code": "ABCD-EFGH", "verify_url": "https://zo.example/device", "expires_in": 600, "interval": 1}))
            }
            "/api/auth/device_poll" => {
                if body["device_code"].as_str() != Some(&"D".repeat(43)) {
                    return (200, json!({"ok": true, "status": "expired"}));
                }
                let i = m.polls.min(m.script.len().saturating_sub(1));
                m.polls += 1;
                let mut a = m.script.get(i).cloned().unwrap_or(json!({"status": "expired"}));
                if a["status"] == "approved" {
                    if m.pending_credential.take().is_none() {
                        // the record is gone after the first time
                        return (200, json!({"ok": true, "status": "expired"}));
                    }
                    let c = format!("zd_browser_{:0>41}", m.creds.len());
                    m.creds.push(c.clone());
                    a["credential"] = json!(c);
                }
                a["ok"] = json!(true);
                (200, a)
            }
            "/api/auth/signout" => {
                if let Some(i) = m.creds.iter().position(|c| c == bearer) {
                    m.creds.remove(i);
                    (200, json!({"ok": true}))
                } else {
                    (401, json!({"ok": false, "error": "Not signed in."}))
                }
            }
            _ => (404, json!({"ok": false, "error": "no such call"})),
        }
    }

    /// Returns the address of the mock (http://127.0.0.1:PORT) and its state. The thread ends with the process.
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
                    if let Some((_, full, bearer, body)) = read_request(&mut c) {
                        let (code, v) = handle(&st, &full, &bearer, &body);
                        reply(&mut c, code, &v);
                    }
                });
            }
        });
        (addr, st)
    }

    fn no_sleep(_: u64, _: &AtomicBool) {}

    #[test]
    fn password_flow_against_the_mock() {
        let (addr, st) = start_mock();
        let fb = format!("{addr}/fb");
        // unknown e-mail / short password / bad address
        assert_eq!(sign_in_password_at(&fb, "nobody@x.co", "secret1").unwrap_err(), "Wrong e-mail or password.");
        assert!(sign_up_at(&fb, "ann@x.co", "123").unwrap_err().contains("at least 6"));
        assert!(sign_in_password_at(&fb, "nonsense", "x").is_err());
        // sign up: the account exists, a verification mail went out, the address is not verified yet
        let s = sign_up_at(&fb, "ann@x.co", "secret1").unwrap();
        assert!(s.verification_sent && s.email_verified == Some(false));
        assert_eq!(st.lock().unwrap().verification_mails, 1);
        assert!(sign_up_at(&fb, "ann@x.co", "secret1").unwrap_err().contains("already an account"));
        // wrong password
        assert_eq!(sign_in_password_at(&fb, "ann@x.co", "wrong").unwrap_err(), "Wrong e-mail or password.");
        // not verified: the server refuses the exchange, with its own words
        let s = sign_in_password_at(&fb, "ann@x.co", "secret1").unwrap();
        assert_eq!(s.email_verified, Some(false));
        assert!(exchange_at(&addr, &s.id_token, "My laptop", "desktop").unwrap_err().contains("Verify"));
        send_verification_at(&fb, &s.id_token).unwrap();
        assert_eq!(st.lock().unwrap().verification_mails, 2);
        // reset: the same answer whether the address has an account or not
        reset_password_at(&fb, "ann@x.co").unwrap();
        reset_password_at(&fb, "ghost@x.co").unwrap();
        // verified now
        st.lock().unwrap().users.get_mut("ann@x.co").unwrap().1 = true;
        let s = sign_in_password_at(&fb, "ann@x.co", "secret1").unwrap();
        assert_eq!(s.email_verified, Some(true));
        let l = exchange_at(&addr, &s.id_token, "My laptop", "desktop").unwrap();
        assert!(l.credential.starts_with("zd_") && l.email == "ann@x.co");
        assert!(exchange_at(&addr, "idtok:ghost@x.co", "x", "desktop").is_err());
        assert!(exchange_at(&addr, &s.id_token, "x", "tablet").is_err());
        // the API key was sent with every Firebase call
        assert!(st.lock().unwrap().log.iter().filter(|l| l.starts_with("/fb/")).all(|l| l.ends_with("key=true")));
        // sign out revokes the credential on the server; a second time finds it already gone, which is fine
        signout_at(&addr, &l.credential).unwrap();
        assert!(st.lock().unwrap().creds.is_empty());
        signout_at(&addr, &l.credential).unwrap();
        assert!(signout_at("http://127.0.0.1:9", &l.credential).unwrap_err().contains("Cannot reach"));
        // a server that is not there
        assert!(sign_in_password_at("http://127.0.0.1:9", "a@b.co", "secret1").unwrap_err().contains("Cannot reach"));
        // plain http to anywhere else is refused before anything is sent
        assert!(exchange_at("http://example.com", "t", "n", "cli").is_err());
        assert!(sign_in_password_at("http://example.com/v1", "a@b.co", "secret1").is_err());
    }

    #[test]
    fn device_flow_against_the_mock() {
        let (addr, st) = start_mock();
        let script = |v: Vec<Value>| {
            let mut m = st.lock().unwrap();
            m.script = v;
            m.polls = 0;
            m.pending_credential = Some(String::new());
        };
        // pending, slow_down, approved
        script(vec![json!({"status": "pending"}), json!({"status": "slow_down"}), json!({"status": "approved", "email": "bob@x.co"})]);
        let shown = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        let l = run_device_flow_at(&addr, "Bob's PC", "desktop", |c, u| shown.lock().unwrap().push((c.to_string(), u.to_string())), &cancel, &no_sleep).unwrap();
        assert!(l.credential.starts_with("zd_browser_") && l.email == "bob@x.co");
        assert_eq!(*shown.lock().unwrap(), vec![("ABCD-EFGH".to_string(), "https://zo.example/device".to_string())]);
        assert_eq!(st.lock().unwrap().polls, 3);
        assert!(!st.lock().unwrap().saw_bearer_on_start);
        // the credential is handed over exactly once: asking again finds nothing
        let again = device_poll(&addr, &"D".repeat(43)).unwrap();
        assert_eq!(again, Poll::Expired);
        // denied
        script(vec![json!({"status": "pending"}), json!({"status": "denied"})]);
        let e = run_device_flow_at(&addr, "x", "cli", |_, _| {}, &cancel, &no_sleep).unwrap_err();
        assert!(e.contains("denied"), "{e}");
        // expired
        script(vec![json!({"status": "expired"})]);
        assert!(run_device_flow_at(&addr, "x", "cli", |_, _| {}, &cancel, &no_sleep).unwrap_err().contains("expired"));
        // cancelled while waiting: stops without another poll
        script(vec![json!({"status": "pending"})]);
        let c2 = AtomicBool::new(false);
        let sleeper = |_: u64, c: &AtomicBool| c.store(true, Ordering::SeqCst);
        assert_eq!(run_device_flow_at(&addr, "x", "cli", |_, _| {}, &c2, &sleeper).unwrap_err(), "Cancelled.");
        assert_eq!(st.lock().unwrap().polls, 0);
        // cancelled before it starts: the server is not even asked
        let starts = st.lock().unwrap().start_calls;
        assert_eq!(run_device_flow_at(&addr, "x", "cli", |_, _| {}, &AtomicBool::new(true), &no_sleep).unwrap_err(), "Cancelled.");
        assert_eq!(st.lock().unwrap().start_calls, starts);
        // a server that is down: the start fails with a friendly text
        assert!(run_device_flow_at("http://127.0.0.1:9", "x", "cli", |_, _| {}, &cancel, &no_sleep).unwrap_err().contains("Cannot reach"));
        // bad kind
        assert!(run_device_flow_at(&addr, "x", "phone", |_, _| {}, &cancel, &no_sleep).is_err());
    }

    // ---- persistence ------------------------------------------------------------------------------------------------------

    fn sandbox(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("zo-login-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_and_clear_login_round_trip() {
        let _g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = sandbox("persist");
        std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
        std::env::set_var("XDG_DATA_HOME", d.join("data"));
        std::env::set_var("ZOHARA_AGENT_KEYS_FILE", d.join("keys.json"));
        std::env::remove_var("ZOHARA_AGENT_KEY_SYNC_TOKEN");
        std::env::remove_var("ZO_SERVER_URL");
        assert_eq!(signed_in(), None);
        assert!(!sync::config().enabled);
        assert_eq!(server_url().unwrap(), DEFAULT_SERVER_URL);
        // a hand-chosen address survives sign-in and sign-out
        sync::save_config("http://127.0.0.1:8799", false).unwrap();
        let l = login();
        save_login(&l).unwrap();
        assert_eq!(signed_in().as_deref(), Some("a@b.co"));
        let c = sync::config();
        assert!(c.enabled && c.email == "a@b.co" && c.url == "http://127.0.0.1:8799");
        assert_eq!(sync::token().as_deref(), Some(l.credential.as_str()));
        assert_eq!(server_url().unwrap(), "http://127.0.0.1:8799");
        // the credential is in the key store, never in sync.json
        let cfg_text = std::fs::read_to_string(d.join("config/zohara-agent/sync.json")).unwrap();
        assert!(!cfg_text.contains("zd_") && !cfg_text.contains("AAAA"), "{cfg_text}");
        // turning syncing off by hand keeps the e-mail
        sync::save_config("http://127.0.0.1:8799", false).unwrap();
        assert_eq!(sync::config().email, "a@b.co");
        // a status from an earlier round is forgotten at sign-out
        sync::reset_status();
        clear_login();
        assert_eq!(signed_in(), None);
        assert!(sync::token().is_none());
        let c = sync::config();
        assert!(!c.enabled && c.email.is_empty());
        assert_eq!(sync::status().last_ok, 0);
        // not a credential: refused, nothing stored
        assert!(save_login(&Login { credential: "nope".into(), email: "a@b.co".into() }).is_err());
        assert_eq!(signed_in(), None);
        // the environment address wins over the saved one
        std::env::set_var("ZO_SERVER_URL", "https://env.example/");
        assert_eq!(server_url().unwrap(), "https://env.example");
        std::env::remove_var("ZO_SERVER_URL");
        for k in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "ZOHARA_AGENT_KEYS_FILE"] {
            std::env::remove_var(k);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn end_to_end_sign_in_save_sync_status_and_sign_out() {
        let _g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (addr, st) = start_mock();
        let d = sandbox("e2e");
        std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
        std::env::set_var("XDG_DATA_HOME", d.join("data"));
        std::env::set_var("ZOHARA_AGENT_KEYS_FILE", d.join("keys.json"));
        std::env::remove_var("ZOHARA_AGENT_KEY_SYNC_TOKEN");
        std::env::set_var("ZO_SERVER_URL", &addr);
        st.lock().unwrap().users.insert("cy@x.co".into(), ("secret1".into(), true));
        let fb = format!("{addr}/fb");
        let s = sign_in_password_at(&fb, "cy@x.co", "secret1").unwrap();
        let l = exchange(&s.id_token, &default_device_name("desktop"), "desktop").unwrap();
        save_login(&l).unwrap();
        assert_eq!(signed_in().as_deref(), Some("cy@x.co"));
        // a sync round now talks to the server with that credential: the mock has no /api/sync/manifest, so it fails with the
        // server's own answer, but it was found and tried (status is recorded)
        let r = sync::sync_now();
        assert!(r.is_err());
        assert!(sync::status().error.contains("no such call") || sync::status().error.contains("server"), "{:?}", sync::status().error);
        // sign out: the credential is revoked on the server and forgotten here
        assert!(sign_out().is_ok());
        assert!(st.lock().unwrap().creds.is_empty());
        assert_eq!(signed_in(), None);
        assert_eq!(sync::status().error, "");
        // sign out with nobody signed in is fine
        assert!(sign_out().is_ok());
        // sign out when the server is gone: signed out here anyway, and the person is told
        save_login(&l).unwrap();
        std::env::set_var("ZO_SERVER_URL", "http://127.0.0.1:9");
        let e = sign_out().unwrap_err();
        assert!(e.contains("Signed out here") && !e.contains("zd_"), "{e}");
        assert_eq!(signed_in(), None);
        for k in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "ZOHARA_AGENT_KEYS_FILE", "ZO_SERVER_URL"] {
            std::env::remove_var(k);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    fn env_on(d: &std::path::Path) {
        std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
        std::env::set_var("XDG_DATA_HOME", d.join("data"));
        std::env::set_var("ZOHARA_AGENT_KEYS_FILE", d.join("keys.json"));
        std::env::remove_var("ZOHARA_AGENT_KEY_SYNC_TOKEN");
        std::env::set_var("ZO_SERVER_URL", "http://127.0.0.1:9");
    }
    fn env_off() {
        for k in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "ZOHARA_AGENT_KEYS_FILE", "ZO_SERVER_URL"] {
            std::env::remove_var(k);
        }
    }

    #[test]
    fn the_desktop_and_the_terminal_have_their_own_sign_ins() {
        let _g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = sandbox("kinds");
        env_on(&d);
        let l = login();
        save_login_as(Client::Desktop, &l).unwrap();
        assert_eq!(signed_in_as(Client::Cli), None, "the terminal is not signed in by the app");
        let cli = Login { credential: format!("zd_{}", "B".repeat(43)), email: "a@b.co".into() };
        save_login_as(Client::Cli, &cli).unwrap();
        assert_eq!(credential_of(Client::Cli).as_deref(), Some(cli.credential.as_str()));
        assert_eq!(credential_of(Client::Desktop).as_deref(), Some(l.credential.as_str()));
        // signing the terminal out leaves the app signed in, and the other way round
        let _ = sign_out_as(Client::Cli);
        assert_eq!(signed_in_as(Client::Cli), None);
        assert_eq!(signed_in().as_deref(), Some("a@b.co"));
        save_login_as(Client::Cli, &cli).unwrap();
        let _ = sign_out();
        assert_eq!(signed_in(), None);
        assert_eq!(signed_in_as(Client::Cli).as_deref(), Some("a@b.co"));
        env_off();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_old_setup_without_an_account_is_switched_off_and_the_screen_check_needs_no_keyring() {
        let _g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = sandbox("legacy");
        env_on(&d);
        sync::write_config("", true, "").unwrap();
        sync::set_token(&format!("zd_{}", "C".repeat(43))).unwrap();
        assert!(sync::token().is_some());
        assert_eq!(signed_in_quick(), None);
        assert!(drop_legacy_setup());
        assert!(sync::token().is_none() && !sync::config().enabled);
        assert!(!drop_legacy_setup(), "nothing left to drop");
        // a real sign-in is left alone
        save_login(&login()).unwrap();
        assert!(!drop_legacy_setup());
        assert_eq!(signed_in_quick().as_deref(), Some("a@b.co"));
        env_off();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_credential_the_server_refuses_is_forgotten_here() {
        let _g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = sandbox("revoked");
        env_on(&d);
        let ln = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", ln.local_addr().unwrap());
        std::thread::spawn(move || {
            for s in ln.incoming().flatten() {
                let mut s = s;
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
            }
        });
        std::env::set_var("ZO_SERVER_URL", &addr);
        save_login(&login()).unwrap();
        assert!(signed_in().is_some());
        assert!(sync::sync_now().is_err());
        assert_eq!(signed_in(), None, "the sign-in is gone");
        assert!(!sync::config().enabled && sync::token().is_none());
        env_off();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn without_a_keyring_the_credential_goes_to_a_private_file() {
        use std::os::unix::fs::PermissionsExt;
        let _g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = sandbox("nokeyring");
        env_on(&d);
        std::env::remove_var("ZOHARA_AGENT_KEYS_FILE");
        std::env::set_var("ZO_TEST_SECRET_TOOL", d.join("nothing")); // no secret-tool (PATH stays untouched: other tests start programs)
        let l = login();
        let r = save_login_as(Client::Cli, &l);
        let got = credential_of(Client::Cli);
        let f = d.join("config/zohara-agent/credentials.json");
        let mode = std::fs::metadata(&f).map(|m| m.permissions().mode() & 0o777);
        clear_login_as(Client::Cli);
        let gone = credential_of(Client::Cli).is_none() && !f.exists();
        std::env::remove_var("ZO_TEST_SECRET_TOOL");
        env_off();
        r.unwrap();
        assert_eq!(got.as_deref(), Some(l.credential.as_str()));
        assert_eq!(mode.unwrap(), 0o600);
        assert!(gone);
        let _ = std::fs::remove_dir_all(&d);
    }
}
