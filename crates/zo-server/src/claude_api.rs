//! The guided Claude Code sign-in on the server, as small calls for the web page (`/api/claude/<call>`). The program, its sandbox and
//! the rules are in `zo_core::claude_login`; here are the people and the limits.
//!
//! Who: an admin, or anybody when the operator set `ZO_CLAUDE_CODE_USERS=all`. Everybody else is refused. The page only ever receives a
//! state, a message and the authorization link: never what the program prints, and never a token. Rates: per person, so that polling
//! cannot be used to hammer the server.

use crate::tenants::Bucket;
use crate::{App, Fail, Who};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tiny_http::Request;
use zo_core::ai::claude_code;
use zo_core::claude_login as login;
use zo_core::tenant;

pub const CALLS: [&str; 6] = ["status", "start", "poll", "code", "cancel", "logout"];
const CALLS_PER_MIN: u32 = 120;

fn buckets() -> &'static Mutex<HashMap<String, Bucket>> {
    static B: std::sync::OnceLock<Mutex<HashMap<String, Bucket>>> = std::sync::OnceLock::new();
    B.get_or_init(|| Mutex::new(HashMap::new()))
}

fn rate(tenant: &str) -> Result<(), Fail> {
    let mut g = buckets().lock().unwrap_or_else(|e| e.into_inner());
    if g.len() > 5000 {
        g.clear();
    }
    g.entry(tenant.to_string()).or_insert_with(|| Bucket::new(CALLS_PER_MIN, Instant::now())).take(Instant::now()).map_err(|w| Fail { status: 429, error: "Too many requests. Wait a moment.".into(), retry: Some(w) })
}

/// May this person sign in to Claude Code on the server?
pub fn allowed(who: &Who) -> bool {
    who.admin || claude_code::users_all()
}

pub fn handle(app: &App, req: Request, name: &str, v: &Value, who: &Who) {
    match run(app, name, v, who) {
        Ok(body) => crate::reply(app, req, 200, body),
        Err(f) => crate::fail(app, req, f),
    }
}

fn run(app: &App, name: &str, v: &Value, who: &Who) -> Result<Value, Fail> {
    rate(&who.tenant)?;
    // what the person may do here, for the page (also for those who may not, so it can say so)
    if name == "status" {
        let lease = app.tenants.acquire(&who.tenant, who.admin, Instant::now())?;
        let me = lease.tenant(!who.admin);
        let st = tenant::scope(&me, || json!({"source": claude_code::token_source(), "set_at": claude_code::token_set_at(), "installed": claude_code::binary().is_some()}));
        return Ok(json!({
            "ok": true,
            "allowed": allowed(who),
            "admin": who.admin,
            "users": if claude_code::users_all() { "all" } else { "admins" },
            "present": st["source"] != "none",
            "source": st["source"], "set_at": st["set_at"], "installed": st["installed"],
            "flow": if allowed(who) { login::server_view_for(&who.tenant).to_json() } else { Value::Null },
        }));
    }
    if !allowed(who) {
        return Err(Fail::new(403, "Signing in to Claude Code on this server is only for the owner's account. Sign in to Claude Code on your own computer and use it from the web through My computer."));
    }
    let fail400 = |e: String| Fail::new(400, e);
    match name {
        "start" => {
            let lease = app.tenants.acquire(&who.tenant, who.admin, Instant::now())?;
            let me = lease.tenant(!who.admin);
            tenant::scope(&me, login::server_start).map(|v| v.to_json()).map_err(fail400)
        }
        "poll" => Ok(login::server_view_for(&who.tenant).to_json()),
        "code" => login::server_submit_for(&who.tenant, v.get("code").and_then(Value::as_str).unwrap_or("")).map(|v| v.to_json()).map_err(fail400),
        "cancel" => {
            login::server_cancel_for(&who.tenant);
            Ok(json!({"ok": true}))
        }
        "logout" => {
            let lease = app.tenants.acquire(&who.tenant, who.admin, Instant::now())?;
            let me = lease.tenant(!who.admin);
            tenant::scope(&me, login::server_logout).map(|_| json!({"ok": true})).map_err(|e| Fail::new(503, e))
        }
        _ => Err(Fail::new(404, "not found")),
    }
}
