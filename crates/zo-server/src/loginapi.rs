//! /api/auth/*: signing a device in. Two public calls (`device_start`, `device_poll`: the device has nothing to prove yet; they are
//! limited per source address and only ever answer about the device's own secret code), three for a signed-in person
//! (`device_lookup`, `device_approve`, `exchange`: a Firebase sign-in with a verified e-mail address, the terms accepted), and
//! `signout` (a device signs itself out). See `devicelogin.rs` for the flow and the store of waiting requests.
//!
//! Nothing here logs a device code, a user code or a credential.

use crate::auth::email_hash;
use crate::devicelogin::{CodeError, Polled, StartError, EXPIRES_SECS, INTERVAL_SECS};
use crate::devices::{strip_name, Kind, MakeErr, MAX_DEVICES};
use crate::{App, Cred, Fail, Who};
use serde_json::{json, Value};

pub const START: &str = "device_start";
pub const POLL: &str = "device_poll";
/// The calls that need a signed-in person.
pub const PERSON_CALLS: [&str; 3] = ["device_lookup", "device_approve", "exchange"];

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn storage(e: impl std::fmt::Display) -> Fail {
    // the message is about the database, never about a code or a credential
    eprintln!("zo-server: sign-in storage: {e}");
    Fail::new(503, "The storage is not reachable right now. Try again in a moment.")
}

/// What the name a device gives itself must look like: 1 to 60 characters once it is cleaned of control characters.
fn device_name(v: &Value) -> Result<String, Fail> {
    let name = strip_name(text(v, "name"));
    if name.is_empty() || name.chars().count() > 60 {
        return Err(Fail::new(400, "The device name must be 1 to 60 characters."));
    }
    Ok(name)
}

fn kind(v: &Value) -> Result<Kind, Fail> {
    Kind::parse(text(v, "kind")).ok_or_else(|| Fail::new(400, "The kind must be \"desktop\" or \"cli\"."))
}

fn locked(e: CodeError) -> Fail {
    match e {
        CodeError::Locked(s) => Fail { status: 429, error: "Too many wrong codes. Wait a few minutes before trying again.".into(), retry: Some(s) },
        CodeError::Unknown => Fail::new(404, "That code is not right, or it has run out. Check the code on your device, or start again there."),
    }
}

/// Credentials whose request ran out before the device collected them are revoked (the person never got them, so nobody has them).
pub fn revoke_orphans(app: &App) {
    let Some(devs) = &app.devices else { return };
    for (tenant, id) in app.login.take_orphans() {
        if let Err(e) = devs.revoke(&tenant, &id) {
            eprintln!("zo-server: an uncollected device credential could not be revoked yet: {e}");
        }
    }
}

/// Housekeeping, every half minute.
pub fn sweep(app: &App) {
    app.login.sweep_now(app.login.now());
    revoke_orphans(app);
}

/// `device_start` and `device_poll`: no login. `ip` is where the request came from.
pub fn public(app: &App, name: &str, v: &Value, ip: &str) -> Result<Value, Fail> {
    if app.devices.is_none() {
        return Err(Fail::new(400, "Signing in a device needs the cloud database, which this server does not have."));
    }
    let now = app.login.now();
    let out = match name {
        START => {
            let (name, kind) = (device_name(v)?, kind(v)?);
            let Some(base) = &app.public_url else { return Err(Fail::new(503, "This server does not know its own public address, so it cannot sign devices in.")) };
            let s = app.login.start(&name, kind, ip, now).map_err(|e| match e {
                StartError::Rate(s) => Fail { status: 429, error: "Too many sign-in requests from your network. Wait a minute.".into(), retry: Some(s) },
                StartError::TooManyHere => Fail { status: 429, error: "Too many sign-in requests are waiting from your network. Finish them, or wait ten minutes.".into(), retry: Some(60) },
                StartError::Full => Fail { status: 503, error: "Too many people are signing in right now. Try again in a minute.".into(), retry: Some(30) },
                StartError::NoRandom(e) => storage(e),
            })?;
            json!({"ok": true, "device_code": s.device_code, "user_code": s.user_code, "verify_url": format!("{base}/device"), "expires_in": EXPIRES_SECS, "interval": INTERVAL_SECS})
        }
        POLL => match app.login.poll(text(v, "device_code"), ip, now) {
            Err(wait) => return Err(Fail { status: 429, error: "You are polling too fast. Wait a moment.".into(), retry: Some(wait) }),
            Ok(Polled::Pending) => json!({"ok": true, "status": "pending"}),
            Ok(Polled::SlowDown) => json!({"ok": true, "status": "slow_down"}),
            Ok(Polled::Denied) => json!({"ok": true, "status": "denied"}),
            Ok(Polled::Expired) => json!({"ok": true, "status": "expired"}),
            Ok(Polled::Approved { credential, email }) => json!({"ok": true, "status": "approved", "credential": credential, "email": email}),
        },
        _ => return Err(Fail::new(404, "not found")),
    };
    revoke_orphans(app);
    Ok(out)
}

fn account(who: &Who) -> Result<(&str, &str), Fail> {
    match &who.cred {
        Cred::Account { uid, email } => Ok((uid, email)),
        _ => Err(Fail::new(403, "Sign in with your account first.")),
    }
}

fn full_message() -> String {
    format!("You have {MAX_DEVICES} signed-in devices already. Sign one out first: open the Account page and remove a device.")
}

/// The calls of a signed-in person, and `signout`.
pub fn handle(app: &App, name: &str, v: &Value, who: &Who) -> Result<Value, Fail> {
    let Some(devs) = &app.devices else { return Err(Fail::new(400, "Signing in a device needs the cloud database, which this server does not have.")) };
    let now = app.login.now();
    let out = match name {
        "device_lookup" => {
            account(who)?;
            let i = app.login.lookup(&who.tenant, text(v, "user_code"), now).map_err(locked)?;
            json!({"ok": true, "name": i.name, "kind": i.kind.name(), "requested_at": i.requested_at, "ip": i.ip})
        }
        "device_approve" => {
            let (_, email) = account(who)?;
            let Some(approve) = v.get("approve").and_then(Value::as_bool) else { return Err(Fail::new(400, "Say whether to approve: {\"approve\": true} or {\"approve\": false}.")) };
            if !approve {
                app.login.deny(&who.tenant, text(v, "user_code"), now).map_err(locked)?;
                revoke_orphans(app);
                return Ok(json!({"ok": true}));
            }
            let claim = app.login.begin_approve(&who.tenant, text(v, "user_code"), now).map_err(locked)?;
            match devs.create(&who.tenant, &email_hash(email), !who.admin, &claim.name, claim.kind) {
                Ok((credential, dev)) => {
                    if !app.login.finish_approve(&claim, credential, email.to_string(), &who.tenant, dev.id.clone(), app.login.now()) {
                        // the request ran out while the credential was made: nobody can collect it
                        let _ = devs.revoke(&who.tenant, &dev.id);
                        return Err(Fail::new(410, "That sign-in request ran out. Start again on your device."));
                    }
                }
                Err(e) => {
                    app.login.abort_approve(&claim);
                    return Err(match e {
                        MakeErr::Full => Fail::new(409, full_message()),
                        MakeErr::Storage(e) => storage(e),
                    });
                }
            }
            json!({"ok": true})
        }
        "exchange" => {
            // a device that signed in with e-mail and password (Firebase's own API) trades that for its own credential
            let (_, email) = account(who)?;
            let (name, kind) = (device_name(v)?, kind(v)?);
            match devs.create(&who.tenant, &email_hash(email), !who.admin, &name, kind) {
                Ok((credential, _)) => json!({"ok": true, "credential": credential, "email": email}),
                Err(MakeErr::Full) => return Err(Fail::new(409, full_message())),
                Err(MakeErr::Storage(e)) => return Err(storage(e)),
            }
        }
        "signout" => match &who.cred {
            // a device signs itself out: its own credential stops working at once
            Cred::Device { id, .. } => {
                let revoked = devs.revoke(&who.tenant, id).map_err(storage)?;
                json!({"ok": true, "revoked": revoked})
            }
            // a person signed in with Firebase can name one of their own devices; without one there is nothing to revoke
            Cred::Account { .. } => {
                let id = text(v, "id");
                let revoked = !id.is_empty() && devs.revoke(&who.tenant, id).map_err(storage)?;
                json!({"ok": true, "revoked": revoked})
            }
        },
        _ => return Err(Fail::new(404, "not found")),
    };
    revoke_orphans(app);
    Ok(out)
}
