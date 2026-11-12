//! /api/account/*: what a person can do about their own account. Always about the caller's own tenant (taken from the verified
//! token, never from the request) and only for a signed-in person (a device token cannot reach any of this).

use crate::tenants::{Lease, Registry};
use crate::{App, Cred, Who};
use serde_json::{json, Value};
use zo_core::tenant::{self, Tenant};

/// The names under /api/account/.
/// There is no call that makes a device credential here: they only come from the device login and `/api/auth/exchange`.
pub const CALLS: [&str; 11] = ["whoami", "devices_list", "device_revoke", "export", "delete", "accept_terms", "push_config", "push_subscribe", "push_unsubscribe", "push_list", "claude_status"];

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The limits that apply to this person, as the Account page shows them.
pub fn limits(admin: bool, per_min: u32) -> Value {
    let mut l = json!({"requests_per_min": per_min, "concurrent_sends": crate::tenants::SENDS_PER_TENANT});
    if !admin {
        l["message_chars"] = json!(tenant::MAX_MESSAGE_CHARS);
        l["data_bytes"] = json!(tenant::MAX_DATA_BYTES);
        l["tool_steps"] = json!(tenant::MAX_STEPS);
        l["answer_secs"] = json!(tenant::MAX_ANSWER_SECS);
    }
    l
}

/// Everything of the person that is worth taking along: chats, memory notes and personalities. Never keys, tokens or logs.
pub fn export(t: &Tenant) -> Value {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let chats = match read(t.data.join("chats.json")) {
        Some(Value::Array(a)) => a,
        _ => Vec::new(),
    };
    let (mut memory, mut memory_settings) = (Vec::new(), json!({}));
    if let Some(Value::Object(mut m)) = read(t.data.join("memory.json")) {
        if let Some(Value::Array(items)) = m.remove("items") {
            memory = items;
        }
        for k in ["enabled", "every"] {
            if let Some(v) = m.remove(k) {
                memory_settings[k] = v;
            }
        }
    }
    let personalities = match read(t.config.join("personalities.json")) {
        Some(v) => v,
        None => json!({}),
    };
    json!({"ok": true, "format": "zo-ai-export-1", "exported_at": now_secs(), "chats": chats, "memory": memory, "memory_settings": memory_settings, "personalities": personalities})
}

/// Whether the person has a Claude sign-in on the server (never the token itself).
pub fn claude_status(who: &Who, me: &Tenant) -> Value {
    let (source, set_at) = tenant::scope(me, || (zo_core::ai::claude_code::token_source(), zo_core::ai::claude_code::token_set_at()));
    json!({"ok": true, "present": source != "none", "source": source, "set_at": set_at, "admin": who.admin})
}

/// /api/sync/claude_token (a desktop's credential): the owner's desktop sends the long-lived token of `claude setup-token`, or
/// removes it. Only an admin account may keep one; it is stored encrypted in that person's own key store and nowhere else. Body:
/// `{"token": "..."}` (or `{"action": "set", "token": "..."}`), `{"delete": true}` (or `{"action": "delete"}`), `{"action": "status"}`.
pub fn claude_token(v: &Value, who: &Who, me: &Tenant) -> (u16, Value) {
    let err = |status: u16, e: &str| (status, json!({"ok": false, "error": e}));
    if !who.admin {
        return err(403, "Only the owner's account can keep a Claude sign-in on the server. Everybody else signs in to Claude Code on their own computer and uses it from the web through My computer.");
    }
    let action = match (text(v, "action"), v.get("delete").and_then(Value::as_bool), v.get("token").is_some()) {
        ("", Some(true), _) => "delete",
        ("", _, true) => "set",
        (a, _, _) => a,
    };
    match action {
        "set" => match tenant::scope(me, || zo_core::ai::claude_code::store_token(text(v, "token"))) {
            Ok(at) => (200, json!({"ok": true, "set_at": at})),
            Err(e) => err(400, &e),
        },
        "delete" => match tenant::scope(me, zo_core::ai::claude_code::clear_token) {
            Ok(()) => (200, json!({"ok": true})),
            Err(e) => err(503, &e),
        },
        "status" => (200, claude_status(who, me)),
        _ => err(400, "Send action set, delete or status."),
    }
}

pub fn handle(app: &App, name: &str, v: &Value, who: &Who, lease: &Lease) -> (u16, Value) {
    let err = |status: u16, e: &str| (status, json!({"ok": false, "error": e}));
    let id = &who.tenant;
    let me = lease.tenant(!who.admin);
    match name {
        "whoami" => {
            let used = tenant::scope(&me, tenant::used_bytes);
            let email = match &who.cred {
                Cred::Account { email, .. } => email.clone(),
                _ => String::new(),
            };
            (200, json!({"ok": true, "admin": who.admin, "restricted": !who.admin, "email": email, "tenant": id, "limits": limits(who.admin, app.tenants.per_min()), "data_bytes": used, "terms": {"version": crate::pages::TERMS_VERSION, "accepted": app.has_accepted(id).unwrap_or(false)}}))
        }
        "accept_terms" => {
            if text(v, "version") != crate::pages::TERMS_VERSION {
                return err(400, "That is not the current version of the notice and the terms. Reload the page.");
            }
            if let Some(db) = app.tenants.db() {
                let rec = json!({"version": crate::pages::TERMS_VERSION, "at": now_secs()});
                if let Err(e) = db.put_doc(&crate::devices::list_collection(id), "terms", &rec.to_string()) {
                    eprintln!("zo-server: {e}");
                    return err(503, "The storage is not reachable right now. Try again in a moment.");
                }
            }
            app.accepted.lock().unwrap().insert(id.clone());
            (200, json!({"ok": true, "version": crate::pages::TERMS_VERSION}))
        }
        "devices_list" => match &app.devices {
            None => (200, json!({"ok": true, "devices": []})),
            Some(d) => match d.list(id) {
                Ok(l) => (200, json!({"ok": true, "devices": l.iter().map(|d| d.public()).collect::<Vec<_>>()})),
                Err(e) => err(503, &e),
            },
        },
        "device_revoke" => {
            let Some(d) = &app.devices else { return err(404, "There is no such device.") };
            match d.revoke(id, text(v, "id")) {
                Ok(true) => {
                    app.link.purge_device(id, text(v, "id"));
                    (200, json!({"ok": true}))
                }
                Ok(false) => err(404, "There is no such device."),
                Err(e) => err(503, &e),
            }
        }
        // notifications when the browser is closed (push.rs): always the caller's own tenant
        "push_config" | "push_subscribe" | "push_unsubscribe" | "push_list" => app.push.account_call(name, v, id),
        "claude_status" => (200, claude_status(who, &me)),
        "export" => (200, export(&me)),
        "delete" => {
            if text(v, "confirm") != "DELETE" {
                return err(400, "Send {\"confirm\": \"DELETE\"} to delete the account.");
            }
            let uid = match &who.cred {
                Cred::Account { uid, .. } => uid.clone(),
                _ => String::new(),
            };
            let mut login_removed = false;
            let r = app.tenants.delete_account(
                lease,
                || app.close_streams(id),
                || {
                    app.accepted.lock().unwrap().remove(id);
                    app.push.forget(id);
                    if let Some(db) = app.tenants.db() {
                        // first the marker that keeps a sign-in token that is still valid from bringing the account back
                        let marker = |removed: bool| json!({"at": now_secs(), "login_removed": removed}).to_string();
                        db.put_doc(crate::tenants::TOMBSTONES, id, &marker(false))?;
                        if let Some(d) = &app.devices {
                            d.purge(id)?;
                        }
                        crate::db::wipe_tree(&**db, &format!("users/{id}"), &["chats", "memory", "files", "meta"])?;
                        if let Some(reg) = app.tenants.registry() {
                            let reg: &Registry = reg;
                            reg.remove(id)?;
                        }
                        // everything of theirs is gone: now their Firebase login (best effort; the data is gone either way)
                        if !uid.is_empty() {
                            // twice before giving up: the person is told plainly if their sign-in record is still there
                            let mut last = Ok(false);
                            for n in 0..2 {
                                last = db.delete_login(&uid);
                                if last.is_ok() {
                                    break;
                                }
                                if n == 0 {
                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                }
                            }
                            match last {
                                Ok(done) => login_removed = done,
                                Err(e) => eprintln!("zo-server: ACCOUNT DELETE: the sign-in record of {id} could not be removed (the data is gone): {e}"),
                            }
                            if login_removed {
                                let _ = db.put_doc(crate::tenants::TOMBSTONES, id, &marker(true));
                            }
                        }
                    }
                    Ok(())
                },
            );
            match r {
                Ok(()) => (200, json!({"ok": true, "login_removed": login_removed})),
                Err(e) => {
                    eprintln!("zo-server: deleting an account did not finish: {e}");
                    err(503, "Deleting did not finish. Nothing more of yours is served; try again in a moment to finish it.")
                }
            }
        }
        _ => err(404, "Unknown account call."),
    }
}
