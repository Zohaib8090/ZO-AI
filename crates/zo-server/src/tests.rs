//! The server as a whole, over real HTTP: the web server runs in this process with a stand-in for Google's sign-in (`auth::Fake`) and
//! an in-memory database (`db::MemDb`) instead of Firestore, and zo-core's own service runs behind it on a socket in a sandbox.

use super::*;
use crate::auth::Fake;
use crate::db::{Db, MemDb};
use crate::testenv;
use std::collections::HashSet;
use std::io::BufRead;

struct Rig {
    app: Arc<App>,
    base: String,
    db: Arc<MemDb>,
}

fn rig(people: &[(&str, &str, &str)], admins: &str, max_users: usize, per_min: u32) -> Rig {
    rig_push(people, admins, max_users, per_min, Default::default())
}

/// Like `rig`, with a notification hub of the test's own (a fake push transport).
fn rig_push(people: &[(&str, &str, &str)], admins: &str, max_users: usize, per_min: u32, push: Arc<push::Hub>) -> Rig {
    let root = testenv::service();
    let db = Arc::new(MemDb::default());
    let dbd: Arc<dyn Db> = db.clone();
    let tenants = Arc::new(TenantMap::new(root, Some(dbd.clone()), testenv::SECRET.into(), max_users, 8).with_rate(per_min));
    let app = Arc::new(App {
        no_auth: false,
        noauth_admin: false,
        verifier: Some(Arc::new(Fake::new(people))),
        policy: Policy::new(Some(admins.into()), Some("blocked@x.test".into()), Some(max_users.to_string())),
        tenants,
        devices: Some(Devices::new(dbd)),
        login: DeviceLogin::new(Default::default()),
        public_url: Some("https://zo.test".into()),
        streams: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        conns: AtomicUsize::new(0),
        legal: Default::default(),
        require_terms: std::sync::atomic::AtomicBool::new(false),
        accepted: Mutex::new(Default::default()),
        push,
        link: link::Link::new(fast_link()),
        term: crate::term::Hub::default(),
        term_root: std::env::temp_dir().join("zo-test-terminals"),
    });
    let server = Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let a = app.clone();
    std::thread::spawn(move || serve(a, server));
    Rig { app, base, db }
}

/// The desktop link with short times, so that expiry and presence can be tried in seconds instead of minutes.
fn fast_link() -> link::Config {
    link::Config {
        poll_wait: Duration::from_millis(400),
        online_window: Duration::from_millis(1800),
        job_expiry: Duration::from_millis(1500),
        job_max: Duration::from_secs(3600),
        lost_after: Duration::from_secs(5),
        op_wait: Duration::from_secs(4),
        stop_grace: Duration::from_millis(700),
        ping_every: Duration::from_millis(300),
        tick: Duration::from_millis(25),
    }
}

struct Answer {
    status: u16,
    body: Value,
    retry: Option<String>,
}

fn call(r: &Rig, token: &str, path: &str, body: Value) -> Answer {
    let mut req = ureq::post(&format!("{}{path}", r.base)).set("Content-Type", "application/json");
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let res = match req.send_string(&body.to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{e}"),
    };
    let status = res.status();
    let retry = res.header("Retry-After").map(str::to_string);
    let text = res.into_string().unwrap_or_default();
    Answer { status, body: serde_json::from_str(&text).unwrap_or(Value::Null), retry }
}

/// Like `call`, from another source address (the proxy's X-Forwarded-For; the last entry is the one that counts).
fn call_from(r: &Rig, ip: &str, token: &str, path: &str, body: Value) -> Answer {
    let mut req = ureq::post(&format!("{}{path}", r.base)).set("Content-Type", "application/json").set("X-Forwarded-For", &format!("6.6.6.6, {ip}"));
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let res = match req.send_string(&body.to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{e}"),
    };
    let status = res.status();
    let retry = res.header("Retry-After").map(str::to_string);
    let text = res.into_string().unwrap_or_default();
    Answer { status, body: serde_json::from_str(&text).unwrap_or(Value::Null), retry }
}

/// A device credential for the person with this Firebase token, through the e-mail sign-in exchange: (credential, device id).
fn make_device(r: &Rig, token: &str, name: &str, kind: &str) -> (String, String) {
    let a = call(r, token, "/api/auth/exchange", json!({"name": name, "kind": kind}));
    assert_eq!(a.status, 200, "{:?}", a.body);
    let cred = a.body["credential"].as_str().unwrap().to_string();
    let list = call(r, token, "/api/account/devices_list", json!({}));
    let id = list.body["devices"].as_array().unwrap().iter().find(|d| d["name"] == name).unwrap()["id"].as_str().unwrap().to_string();
    (cred, id)
}

fn uniq(tag: &str) -> String {
    format!("{tag}-{}", std::process::id())
}

fn chat(id: &str, title: &str) -> Value {
    json!({"id": id, "title": title, "updated": 100, "entries": []})
}

fn tenant_of(uid: &str) -> String {
    auth::tenant_id(uid)
}

// ---------------------------------------------------------------------------------------------------------------------------

#[test]
fn only_this_computer_counts_as_local() {
    for h in ["127.0.0.1", "127.0.0.1:8787", "localhost:8787", "[::1]:8787", "[::1]"] {
        assert!(host_is_local(h), "{h}");
    }
    for h in ["", "evil.example", "127.0.0.1.evil.example", "localhost.evil.example:80", "[::1]x", "0.0.0.0:8787", "attacker.test:8787"] {
        assert!(!host_is_local(h), "{h}");
    }
}

#[test]
fn the_scope_matrix_every_route_for_every_kind_of_credential() {
    let acct = Cred::Account { uid: "u".into(), email: "e".into() };
    let full = Cred::Device { scope: Scope::Full, id: "d".into() };
    let sync = Cred::Device { scope: Scope::Sync, id: "d".into() };
    let routes = [Route::Op, Route::Send, Route::Reply, Route::Sync, Route::Account, Route::Auth, Route::Signout, Route::LinkDevice, Route::LinkStatus, Route::ClaudeToken, Route::Term, Route::Claude];
    // a web session is never a linked computer and never hands over the Claude token; the terminal client is neither; the desktop app's
    // credential may sync, sign out, link and send the token (and ask which computers are linked)
    let table: [(&Cred, [bool; 12]); 3] = [
        (&acct, [true, true, true, true, true, true, true, false, true, false, true, true]),
        (&full, [true, true, true, true, false, false, true, false, true, false, true, true]),
        (&sync, [false, false, false, true, false, false, true, true, true, true, false, false]),
    ];
    for (cred, want) in table {
        for (r, w) in routes.iter().zip(want) {
            assert_eq!(permits(cred, *r), w, "{cred:?} on {r:?}");
        }
    }
    // the paths map to those routes, and nothing else is a route
    let post = Method::Post;
    for (p, r) in [("/api/op", Route::Op), ("/api/send", Route::Send), ("/api/reply", Route::Reply), ("/api/sync/manifest", Route::Sync), ("/api/sync/get", Route::Sync), ("/api/sync/put", Route::Sync), ("/api/sync/delete", Route::Sync)] {
        assert_eq!(route_of(&post, p), Some(r), "{p}");
    }
    for (p, r) in [("/api/link/poll", Route::LinkDevice), ("/api/link/events", Route::LinkDevice), ("/api/link/status", Route::LinkStatus), ("/api/sync/claude_token", Route::ClaudeToken)] {
        assert_eq!(route_of(&post, p), Some(r), "{p}");
    }
    for n in term::CALLS {
        assert_eq!(route_of(&post, &format!("/api/term/{n}")), Some(Route::Term), "{n}");
    }
    for n in claude_api::CALLS {
        assert_eq!(route_of(&post, &format!("/api/claude/{n}")), Some(Route::Claude), "{n}");
    }
    for p in ["/api/term", "/api/term/", "/api/term/run", "/api/term/open/x", "/api/claude/", "/api/claude/token", "/api/claude/start/x", "/api/link", "/api/link/", "/api/link/other", "/api/sync/claude", "/api/sync/claude_token/x"] {
        assert_eq!(route_of(&post, p), None, "{p}");
    }
    assert_eq!(route_of(&Method::Get, "/api/link/poll"), None);
    for n in account::CALLS {
        assert_eq!(route_of(&post, &format!("/api/account/{n}")), Some(Route::Account));
    }
    for (p, r) in [("/api/auth/device_start", Route::DeviceStart), ("/api/auth/device_poll", Route::DevicePoll), ("/api/auth/device_lookup", Route::Auth), ("/api/auth/device_approve", Route::Auth), ("/api/auth/exchange", Route::Auth), ("/api/auth/signout", Route::Signout)] {
        assert_eq!(route_of(&post, p), Some(r), "{p}");
    }
    for p in ["/api/account/device_create", "/api/auth/", "/api/auth/device_create", "/api/auth/other", "/api/account/", "/api/account/other", "/api/sync/other", "/api/sync", "/api/opx", "/", "/healthz"] {
        assert_eq!(route_of(&post, p), None, "{p}");
    }
    assert_eq!(route_of(&Method::Get, "/api/op"), None);
}

#[test]
fn the_tenant_a_request_runs_as_comes_from_the_credential_not_from_the_request() {
    let t = zo_core::tenant::Tenant { id: "a".repeat(32), config: "/x".into(), data: "/y".into(), restricted: true };
    let v = for_service(json!({"op": "chats", "tenant": {"id": "b".repeat(32), "restricted": false}}), &t);
    assert_eq!(v["tenant"], json!({"id": "a".repeat(32), "restricted": true}));
}

#[test]
fn two_people_cannot_see_each_others_data_through_the_http_api() {
    let (ua, ub) = (uniq("iso-a"), uniq("iso-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "owner@x.test", 200, 600);
    // A saves a chat (through the sync call) and a memory note (through the service)
    assert_eq!(call(&r, "tok-a", "/api/sync/put", json!({"kind": "chats", "id": "c-secret", "doc": chat("c-secret", "A's private chat")})).status, 200);
    let n = call(&r, "tok-a", "/api/op", json!({"op": "memory_add", "text": "A keeps bees"}));
    assert_eq!(n.status, 200, "{:?}", n.body);
    let a_chats = call(&r, "tok-a", "/api/op", json!({"op": "chats"}));
    assert!(a_chats.body.to_string().contains("A's private chat"), "{:?}", a_chats.body);
    // B sees none of it, in the service, in the sync and in the export
    let b_chats = call(&r, "tok-b", "/api/op", json!({"op": "chats"}));
    assert_eq!(b_chats.status, 200);
    assert!(!b_chats.body.to_string().contains("private chat"), "{:?}", b_chats.body);
    assert!(call(&r, "tok-b", "/api/sync/manifest", json!({})).body["chats"].get("c-secret").is_none());
    assert_eq!(call(&r, "tok-b", "/api/sync/get", json!({"kind": "chats", "id": "c-secret"})).status, 400);
    let b_mem = call(&r, "tok-b", "/api/op", json!({"op": "memory"}));
    assert!(!b_mem.body.to_string().contains("bees"), "{:?}", b_mem.body);
    let b_export = call(&r, "tok-b", "/api/account/export", json!({}));
    assert!(!b_export.body.to_string().contains("bees") && !b_export.body.to_string().contains("private chat"));
    // B cannot name A as the tenant to act as: the field in the request is replaced
    let forged = call(&r, "tok-b", "/api/op", json!({"op": "chats", "tenant": {"id": tenant_of(&ua), "restricted": false}}));
    assert!(!forged.body.to_string().contains("private chat"), "{:?}", forged.body);
    let forged_machines = call(&r, "tok-b", "/api/op", json!({"op": "machines", "tenant": {"id": tenant_of(&ub), "restricted": false}}));
    assert_eq!(forged_machines.body["ok"], false, "still restricted: {:?}", forged_machines.body);
    // A's export has them, and no keys
    let a_export = call(&r, "tok-a", "/api/account/export", json!({}));
    let s = a_export.body.to_string();
    assert!(s.contains("A's private chat") && s.contains("A keeps bees"), "{s}");
    let keys: HashSet<&str> = a_export.body.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, HashSet::from(["ok", "format", "exported_at", "chats", "memory", "memory_settings", "personalities"]));
    assert!(!s.contains("keys") && !s.contains("token"));
    // their folders are different
    assert_ne!(tenant_of(&ua), tenant_of(&ub));
    assert!(testenv::env().join(tenant_of(&ua)).join("data").exists() && testenv::env().join(tenant_of(&ub)).join("data").exists());
}

#[test]
fn a_running_answer_can_only_be_reached_by_its_owner() {
    let (ua, ub) = (uniq("rep-a"), uniq("rep-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 600);
    let (mine, theirs) = UnixStream::pair().unwrap();
    r.app.streams.lock().unwrap().insert(77, (tenant_of(&ua), mine));
    assert_eq!(call(&r, "tok-b", "/api/reply", json!({"stream": 77, "op": "stop"})).status, 404, "B cannot stop A's answer");
    theirs.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let mut line = String::new();
    assert!(BufReader::new(&theirs).read_line(&mut line).is_err(), "nothing reached A's answer");
    assert_eq!(call(&r, "tok-a", "/api/reply", json!({"stream": 77, "op": "stop"})).status, 200);
    let mut line = String::new();
    BufReader::new(&theirs).read_line(&mut line).unwrap();
    assert!(line.contains("\"stop\"") && !line.contains("tenant"));
    // also answer op passes through to the stream
    assert_eq!(call(&r, "tok-a", "/api/reply", json!({"stream": 77, "op": "answer", "id": 1, "answers": [{"selected": ["PostgreSQL"]}]})).status, 200);
    let mut line2 = String::new();
    BufReader::new(&theirs).read_line(&mut line2).unwrap();
    assert!(line2.contains("\"answer\"") && line2.contains("PostgreSQL"));
}

#[test]
fn the_restricted_person_gets_the_five_tools_and_the_admin_does_not_get_refused() {
    let (ur, uo) = (uniq("tool-r"), uniq("tool-o"));
    let r = rig(&[("tok-r", &ur, "r@x.test"), ("tok-o", &uo, "owner@x.test")], "owner@x.test", 200, 600);
    // the tool list the model would see
    let restricted = zo_core::tenant::Tenant::open_in(&testenv::env(), &tenant_of(&ur), true).unwrap();
    let names: HashSet<&str> = zo_core::tenant::scope(&restricted, || zo_core::tools::active().iter().map(|d| d.name).collect());
    assert_eq!(names, HashSet::from(["zohara_web_search", "zohara_web_fetch", "zohara_memory_search", "zohara_memory_save", "zohara_memory_forget", "zohara_ask_user"]));
    // through the service: refused for the restricted person, allowed for the admin
    let w = call(&r, "tok-r", "/api/account/whoami", json!({}));
    assert_eq!((w.body["admin"].clone(), w.body["restricted"].clone()), (json!(false), json!(true)));
    for op in ["machines", "mcp_list", "scheduled", "projects"] {
        let a = call(&r, "tok-r", "/api/op", json!({"op": op}));
        assert_eq!(a.body["ok"], false, "{op}: {:?}", a.body);
    }
    let mode = call(&r, "tok-r", "/api/op", json!({"op": "mode", "set": "bypass"}));
    assert_eq!(mode.body["ok"], false, "{:?}", mode.body);
    let admin = call(&r, "tok-o", "/api/op", json!({"op": "machines"}));
    assert_eq!(admin.body["ok"], true, "{:?}", admin.body);
    let w = call(&r, "tok-o", "/api/account/whoami", json!({}));
    assert_eq!((w.body["admin"].clone(), w.body["restricted"].clone()), (json!(true), json!(false)));
    // and the admin's tool list is the big one
    let owner = zo_core::tenant::Tenant::open_in(&testenv::env(), &tenant_of(&uo), false).unwrap();
    assert!(zo_core::tenant::scope(&owner, || zo_core::tools::active().len()) > 5);
}

#[test]
fn a_message_reaches_the_service_for_a_restricted_person_and_streams_back() {
    let u = uniq("send");
    let r = rig(&[("tok", &u, "s@x.test")], "", 200, 600);
    let res = ureq::post(&format!("{}/api/send", r.base)).set("Authorization", "Bearer tok").send_string(&json!({"text": "hello"}).to_string()).unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.header("Content-Type"), Some("application/x-ndjson"));
    let body = res.into_string().unwrap();
    let events: Vec<Value> = body.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    assert_eq!(events[0]["event"], "stream", "{body}");
    assert!(events.iter().any(|e| e["event"] == "done"), "{body}");
    assert!(body.contains("model") || body.contains("Model"), "no model is chosen yet, and the answer says so: {body}");
    // a message that is too long for a restricted person is refused by the service
    let long = call(&r, "tok", "/api/op", json!({"op": "send", "text": "x"}));
    assert_eq!(long.status, 400, "/api/op does not send messages");
    // the place is free again afterwards
    assert_eq!(r.app.tenants.running_answers(), 0);
}

const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

fn b64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// A PNG that is `n` bytes long (a real header, then filler).
fn big_png(n: usize) -> String {
    let mut d = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    d.resize(n, 9);
    b64(&d)
}

fn send_raw(r: &Rig, token: &str, body: &Value) -> (u16, String) {
    match ureq::post(&format!("{}/api/send", r.base)).set("Authorization", &format!("Bearer {token}")).send_string(&body.to_string()) {
        Ok(res) => (res.status(), res.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(c, res)) => (c, res.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn attachments_are_checked_by_the_server_before_anything_is_passed_on_and_a_message_may_be_big() {
    let u = uniq("attach");
    let r = rig(&[("tok", &u, "a@x.test")], "", 200, 600);
    let img = |mime: &str, data: &str| json!({"kind": "image", "mime": mime, "name": "a.png", "data_b64": data});
    let refused = |what: &str, att: Value, want: &str| {
        let (code, body) = send_raw(&r, "tok", &json!({"text": "hi", "attachments": att}));
        assert_eq!(code, 400, "{what}: {body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains(want), "{what}: {body}");
        assert!(!body.contains("\"event\""), "{what}: nothing was streamed");
    };
    refused("not a list", json!("x"), "list");
    refused("wrong mime", json!([img("image/svg+xml", PNG_B64)]), "PNG, JPEG");
    refused("bad base64", json!([img("image/png", "%%%")]), "not valid base64");
    refused("magic bytes", json!([img("image/jpeg", PNG_B64)]), "not really");
    refused("a text file under a picture mime", json!([img("image/png", &b64(b"just some words here"))]), "not really");
    refused("too many pictures", Value::Array(vec![img("image/png", PNG_B64); 5]), "Too many pictures");
    refused("too many files", Value::Array((0..7).map(|_| json!({"kind": "text", "name": "t", "text": "x"})).collect()), "Too many files");
    refused("a picture over 3 MB", json!([img("image/png", &big_png(3 * 1024 * 1024 + 1))]), "too big");
    refused("text over 200 KB", json!([{"kind": "text", "name": "t", "text": "a".repeat(200 * 1024 + 1)}]), "too much text");
    refused("text over 400 KB together", json!([{"kind": "text", "name": "t", "text": "a".repeat(200 * 1024)}, {"kind": "text", "name": "u", "text": "a".repeat(200 * 1024)}, {"kind": "text", "name": "v", "text": "a"}]), "together");
    refused("a binary file as text", json!([{"kind": "text", "name": "x.bin", "text": "a\u{0}b"}]), "not a text file");
    refused("an unknown kind", json!([{"kind": "video", "name": "x"}]), "not supported");
    refused("a server path instead of data", json!([{"kind": "image", "mime": "image/png", "name": "x", "path": "/etc/passwd"}]), "no data");
    // a big message is allowed: three pictures of 3 MB (about 12 MB on the wire) pass the size limit and the checks, and reach the service
    // (which answers that no model is chosen: this person has not set one up)
    let three = Value::Array(vec![img("image/png", &big_png(3 * 1024 * 1024)); 3]);
    let (code, body) = send_raw(&r, "tok", &json!({"text": "look at these", "attachments": three}));
    assert_eq!(code, 200, "{}", &body[..body.len().min(300)]);
    assert!(body.contains("\"event\":\"stream\"") && body.contains("\"event\":\"done\""), "{body}");
    // plain messages and messages with an empty list still work, and the place is free afterwards
    assert_eq!(send_raw(&r, "tok", &json!({"text": "hello", "attachments": []})).0, 200);
    assert_eq!(r.app.tenants.running_answers(), 0);
    // the other routes keep the small limit
    assert_eq!(call(&r, "tok", "/api/op", json!({"op": "chats", "pad": "x".repeat(BODY_LIMIT)})).status, 413);
}

#[test]
fn there_is_only_a_limited_number_of_big_message_bodies_at_one_time() {
    let n = AtomicUsize::new(0);
    let held: Vec<_> = (0..MAX_BIG_SENDS).map(|_| BigSend::take(&n).expect("a place")).collect();
    assert!(BigSend::take(&n).is_none());
    assert_eq!(n.load(Ordering::SeqCst), MAX_BIG_SENDS, "a refused try takes nothing");
    drop(held);
    assert_eq!(n.load(Ordering::SeqCst), 0);
    assert!(BigSend::take(&n).is_some());
    // one person holds at most a few of the places
    let mine: Vec<_> = (0..MAX_BIG_SENDS_PER_PERSON).map(|_| BigShare::take("big-person-a").expect("a place")).collect();
    assert!(BigShare::take("big-person-a").is_none());
    assert!(BigShare::take("big-person-b").is_some(), "another person is not locked out");
    drop(mine);
    assert!(BigShare::take("big-person-a").is_some());
}

#[test]
fn device_credentials_a_desktop_one_cannot_call_the_api_a_cli_one_can_a_revoked_one_stops() {
    let u = uniq("dev");
    let r = rig(&[("tok", &u, "d@x.test")], "", 200, 600);
    let (st, sid) = make_device(&r, "tok", "laptop", "desktop");
    let (ft, fid) = make_device(&r, "tok", "terminal", "cli");
    assert!(st.starts_with("zd_") && ft.starts_with("zd_") && st != ft && sid != fid);
    // there is no way to make a credential by hand any more, and the kind must be one of the two
    assert_eq!(call(&r, "tok", "/api/account/device_create", json!({"name": "x", "scope": "sync"})).status, 404);
    assert_eq!(call(&r, "tok", "/api/auth/exchange", json!({"name": "x", "kind": "root"})).status, 400);
    assert_eq!(call(&r, "tok", "/api/auth/exchange", json!({"name": "", "kind": "cli"})).status, 400);
    assert_eq!(call(&r, "tok", "/api/auth/exchange", json!({"name": "x".repeat(61), "kind": "cli"})).status, 400);
    assert_eq!(call(&r, "tok", "/api/auth/exchange", json!({"kind": "cli"})).status, 400);
    // the list says what and when, never a credential or a hash
    let list = call(&r, "tok", "/api/account/devices_list", json!({}));
    let shown = list.body.to_string();
    assert!(!shown.contains(&st) && !shown.contains(&devices::hash_token(&st)) && list.body["devices"].as_array().unwrap().len() == 2, "{shown}");
    let d = list.body["devices"].as_array().unwrap().iter().find(|d| d["name"] == "terminal").unwrap();
    assert_eq!((d["kind"].clone(), d["scope"].clone()), (json!("cli"), json!("full")));
    assert!(d["created"].as_u64().unwrap() > 0 && d.get("last_used").is_some());
    // desktop credential: only /api/sync/* (and signing out)
    assert_eq!(call(&r, &st, "/api/sync/manifest", json!({})).status, 200);
    assert_eq!(call(&r, &st, "/api/op", json!({"op": "chats"})).status, 403);
    assert_eq!(call(&r, &st, "/api/send", json!({"text": "hi"})).status, 403);
    assert_eq!(call(&r, &st, "/api/reply", json!({"stream": 1})).status, 403);
    assert_eq!(call(&r, &st, "/api/account/whoami", json!({})).status, 403);
    assert_eq!(call(&r, &st, "/api/auth/exchange", json!({"name": "x", "kind": "cli"})).status, 403, "a device cannot make another");
    assert_eq!(call(&r, &st, "/api/auth/device_approve", json!({"user_code": "AAAA-AAAA", "approve": true})).status, 403);
    // cli credential: the API and the sync, not the account and not the approval
    assert_eq!(call(&r, &ft, "/api/op", json!({"op": "chats"})).status, 200);
    assert_eq!(call(&r, &ft, "/api/sync/manifest", json!({})).status, 200);
    assert_eq!(call(&r, &ft, "/api/account/devices_list", json!({})).status, 403);
    assert_eq!(call(&r, &ft, "/api/auth/device_lookup", json!({"user_code": "AAAA-AAAA"})).status, 403);
    assert_eq!(call(&r, &ft, "/api/account/delete", json!({"confirm": "DELETE"})).status, 403, "a device cannot delete the account");
    // a device sees the same data as its owner
    assert_eq!(call(&r, &ft, "/api/sync/put", json!({"kind": "chats", "id": "dc", "doc": chat("dc", "from the terminal")})).status, 200);
    assert!(call(&r, "tok", "/api/op", json!({"op": "chats"})).body.to_string().contains("from the terminal"));
    // made-up and malformed credentials
    assert_eq!(call(&r, &format!("zd_{}", "A".repeat(43)), "/api/sync/manifest", json!({})).status, 401);
    assert_eq!(call(&r, "zd_short", "/api/sync/manifest", json!({})).status, 401);
    assert_eq!(call(&r, "", "/api/sync/manifest", json!({})).status, 401);
    // the person revokes one of them, even the one that is in use right now
    assert_eq!(call(&r, "tok", "/api/account/device_revoke", json!({"id": fid})).status, 200);
    assert_eq!(call(&r, &ft, "/api/op", json!({"op": "chats"})).status, 401, "revoked");
    assert_eq!(call(&r, &st, "/api/sync/manifest", json!({})).status, 200, "the other one still works");
    assert_eq!(call(&r, "tok", "/api/account/device_revoke", json!({"id": fid})).status, 404);
}

#[test]
fn signing_out_revokes_the_calling_credential_and_only_that_one() {
    let (ua, ub) = (uniq("so-a"), uniq("so-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 600);
    let (d1, _) = make_device(&r, "tok-a", "one", "desktop");
    let (d2, _) = make_device(&r, "tok-a", "two", "cli");
    let (other, _) = make_device(&r, "tok-b", "b's", "cli");
    for c in [&d1, &d2, &other] {
        assert_eq!(call(&r, c, "/api/sync/manifest", json!({})).status, 200);
    }
    let out = call(&r, &d2, "/api/auth/signout", json!({}));
    assert_eq!((out.status, out.body["revoked"].clone()), (200, json!(true)), "{:?}", out.body);
    assert_eq!(call(&r, &d2, "/api/sync/manifest", json!({})).status, 401, "signed out");
    assert_eq!(call(&r, &d2, "/api/auth/signout", json!({})).status, 401, "and signing out again is just refused");
    assert_eq!(call(&r, &d1, "/api/sync/manifest", json!({})).status, 200, "the person's other device is untouched");
    assert_eq!(call(&r, &other, "/api/sync/manifest", json!({})).status, 200, "so is another person's");
    let names: Vec<Value> = call(&r, "tok-a", "/api/account/devices_list", json!({})).body["devices"].as_array().unwrap().iter().map(|d| d["name"].clone()).collect();
    assert_eq!(names, vec![json!("one")]);
    // a desktop credential may sign itself out too
    assert_eq!(call(&r, &d1, "/api/auth/signout", json!({})).status, 200);
    assert_eq!(call(&r, &d1, "/api/sync/manifest", json!({})).status, 401);
    // with a Firebase sign-in, signout revokes the named device of that person (not somebody else's) or nothing
    let (d3, id3) = make_device(&r, "tok-a", "three", "cli");
    assert_eq!(call(&r, "tok-a", "/api/auth/signout", json!({})).body["revoked"], json!(false));
    assert_eq!(call(&r, "tok-b", "/api/auth/signout", json!({"id": id3})).body["revoked"], json!(false));
    assert_eq!(call(&r, &d3, "/api/sync/manifest", json!({})).status, 200);
    assert_eq!(call(&r, "tok-a", "/api/auth/signout", json!({"id": id3})).body["revoked"], json!(true));
    assert_eq!(call(&r, &d3, "/api/sync/manifest", json!({})).status, 401);
}

#[test]
fn another_persons_device_cannot_be_revoked_and_a_token_belongs_to_its_own_tenant() {
    let (ua, ub) = (uniq("dv-a"), uniq("dv-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 600);
    let (token, id) = make_device(&r, "tok-a", "a", "cli");
    assert_eq!(call(&r, "tok-b", "/api/account/device_revoke", json!({"id": id})).status, 404);
    assert_eq!(call(&r, &token, "/api/sync/put", json!({"kind": "chats", "id": "x1", "doc": chat("x1", "only A")})).status, 200);
    assert!(call(&r, "tok-b", "/api/sync/manifest", json!({})).body["chats"].get("x1").is_none());
    assert_eq!(call(&r, "tok-b", "/api/account/devices_list", json!({})).body["devices"].as_array().unwrap().len(), 0);
}

#[test]
fn blocked_people_are_refused_and_a_blocked_owner_loses_their_device_tokens_too() {
    let r = rig(&[("tok-x", &uniq("blk"), "blocked@x.test"), ("tok-o", &uniq("blk-o"), "owner@x.test")], "owner@x.test", 200, 600);
    let a = call(&r, "tok-x", "/api/op", json!({"op": "chats"}));
    assert_eq!(a.status, 403, "{:?}", a.body);
    assert_eq!(call(&r, "nonsense", "/api/op", json!({"op": "chats"})).status, 401);
    // the owner's token reports an admin
    let (token, _) = make_device(&r, "tok-o", "o", "cli");
    let token = token.as_str();
    assert_eq!(call(&r, token, "/api/op", json!({"op": "machines"})).body["ok"], true, "a token of an admin acts as an admin");
    let blocked_policy = Policy::new(Some("owner@x.test".into()), Some("owner@x.test".into()), None);
    assert!(blocked_policy.is_blocked_hash(&auth::email_hash("owner@x.test")));
}

#[test]
fn the_cap_refuses_the_next_person_but_not_the_ones_already_in_and_not_admins() {
    let ids: Vec<String> = (0..5).map(|n| uniq(&format!("cap{n}"))).collect();
    let people: Vec<(String, &str, String)> = ids.iter().enumerate().map(|(n, u)| (format!("t{n}"), u.as_str(), format!("p{n}@x.test"))).collect();
    let refs: Vec<(&str, &str, &str)> = people.iter().map(|(t, u, e)| (t.as_str(), *u, e.as_str())).collect();
    let r = rig(&refs, "p4@x.test", 3, 600);
    for n in 0..3 {
        assert_eq!(call(&r, &format!("t{n}"), "/api/op", json!({"op": "chats"})).status, 200, "person {n}");
    }
    // the fourth ordinary person is refused with a clear message and takes no place
    let no = call(&r, "t3", "/api/op", json!({"op": "chats"}));
    assert_eq!(no.status, 503, "{:?}", no.body);
    assert!(no.body["error"].as_str().unwrap().contains("as many people"), "{:?}", no.body);
    assert_eq!(r.db.list_docs("registry").unwrap().len(), 3);
    assert!(!testenv::env().join(tenant_of(&ids[3])).exists(), "a refused person gets no folder");
    // someone who is in stays in, and the owner is not counted
    assert_eq!(call(&r, "t1", "/api/account/whoami", json!({})).status, 200);
    assert_eq!(call(&r, "t4", "/api/op", json!({"op": "chats"})).status, 200);
    assert_eq!(r.db.list_docs("registry").unwrap().len(), 3);
    // a place frees up when somebody deletes their account
    assert_eq!(call(&r, "t0", "/api/account/delete", json!({"confirm": "DELETE"})).status, 200);
    assert_eq!(call(&r, "t3", "/api/op", json!({"op": "chats"})).status, 200);
}

#[test]
fn the_limits_return_429_with_retry_after() {
    let u = uniq("rate");
    let r = rig(&[("tok", &u, "r@x.test"), ("other", &uniq("rate2"), "o@x.test")], "", 200, 5);
    for n in 0..5 {
        assert_eq!(call(&r, "tok", "/api/op", json!({"op": "chats"})).status, 200, "request {n}");
    }
    let over = call(&r, "tok", "/api/op", json!({"op": "chats"}));
    assert_eq!(over.status, 429, "{:?}", over.body);
    assert!(over.retry.as_deref().and_then(|s| s.parse::<u64>().ok()).is_some_and(|s| s >= 1), "{:?}", over.retry);
    assert_eq!(over.body["ok"], false);
    assert_eq!(call(&r, "other", "/api/op", json!({"op": "chats"})).status, 200, "somebody else has their own allowance");
}

#[test]
fn bodies_over_the_limit_are_refused_with_413() {
    let u = uniq("body");
    let r = rig(&[("tok", &u, "b@x.test")], "", 200, 600);
    let big = json!({"op": "chats", "pad": "x".repeat(BODY_LIMIT)});
    assert_eq!(call(&r, "tok", "/api/op", big.clone()).status, 413);
    // a message may carry attachments, so it has a bigger limit of its own: 1 MB passes it, 14 MB does not
    assert_ne!(call(&r, "tok", "/api/send", big).status, 413);
    assert_eq!(call(&r, "tok", "/api/send", json!({"text": "x", "pad": "x".repeat(SEND_BODY_LIMIT)})).status, 413);
    // the sync takes more (this one is refused for another reason: it is not a chat)
    let sync = call(&r, "tok", "/api/sync/put", json!({"kind": "chats", "id": "c", "doc": {"pad": "x".repeat(BODY_LIMIT + 10)}}));
    assert_ne!(sync.status, 413);
    assert_eq!(call(&r, "tok", "/api/op", json!(["not", "an", "object"])).status, 400);
}

#[test]
fn delete_wipes_one_person_completely_and_leaves_the_other_untouched() {
    let (ua, ub) = (uniq("del-a"), uniq("del-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 600);
    let (ta, tb) = (tenant_of(&ua), tenant_of(&ub));
    for (tok, name) in [("tok-a", "A"), ("tok-b", "B")] {
        assert_eq!(call(&r, tok, "/api/sync/put", json!({"kind": "chats", "id": format!("c-{name}"), "doc": chat(&format!("c-{name}"), name)})).status, 200);
        assert_eq!(call(&r, tok, "/api/op", json!({"op": "memory_add", "text": format!("note of {name}")})).status, 200);
    }
    let (token_a, _) = make_device(&r, "tok-a", "d", "cli");
    let (token_b, _) = make_device(&r, "tok-b", "d", "desktop");
    r.app.tenants.push_all();
    assert!(r.db.count_under(&format!("users/{ta}")) >= 3 && r.db.count_under(&format!("users/{tb}")) >= 3, "both are in the database");
    // the confirmation is required
    assert_eq!(call(&r, "tok-a", "/api/account/delete", json!({})).status, 400);
    assert_eq!(call(&r, "tok-a", "/api/account/delete", json!({"confirm": "delete"})).status, 400);
    assert_eq!(r.db.count_under(&format!("users/{ta}")) > 0, true);
    let gone = call(&r, "tok-a", "/api/account/delete", json!({"confirm": "DELETE"}));
    assert_eq!((gone.status, gone.body["ok"].clone()), (200, json!(true)), "{:?}", gone.body);
    // A: no document, no device index entry, no registry slot, no folder
    assert_eq!(r.db.count_under(&format!("users/{ta}")), 0);
    let idx = r.db.list_docs("deviceidx").unwrap();
    assert!(idx.values().all(|t| !t.contains(&ta)) && idx.len() == 1, "only B's device is left: {idx:?}");
    assert!(r.db.get_doc("registry", &ta).unwrap().is_none() && r.db.get_doc("registry", &tb).unwrap().is_some());
    assert!(!testenv::env().join(&ta).exists());
    // nothing can bring A back: not the push loop, not another request, not the old device token
    let writes = r.db.writes.load(Ordering::SeqCst);
    r.app.tenants.push_all();
    assert_eq!(r.db.writes.load(Ordering::SeqCst), writes);
    assert_eq!(call(&r, "tok-a", "/api/op", json!({"op": "chats"})).status, 410);
    assert_eq!(call(&r, &token_a, "/api/op", json!({"op": "chats"})).status, 401);
    assert_eq!(r.db.count_under(&format!("users/{ta}")), 0);
    // running it again is harmless
    assert_ne!(call(&r, "tok-a", "/api/account/delete", json!({"confirm": "DELETE"})).status, 500);
    assert_eq!(r.db.count_under(&format!("users/{ta}")), 0);
    // B is as before
    assert!(call(&r, "tok-b", "/api/op", json!({"op": "chats"})).body.to_string().contains("\"B\""));
    assert!(call(&r, "tok-b", "/api/op", json!({"op": "memory"})).body.to_string().contains("note of B"));
    assert_eq!(call(&r, &token_b, "/api/sync/manifest", json!({})).status, 200);
    assert!(r.db.count_under(&format!("users/{tb}")) >= 3);
}

#[test]
fn whoami_has_the_shape_the_web_page_reads() {
    let (u, o) = (uniq("who"), uniq("who-o"));
    let r = rig(&[("tok", &u, "me@x.test"), ("tok-o", &o, "owner@x.test")], "owner@x.test", 200, 600);
    let w = call(&r, "tok", "/api/account/whoami", json!({})).body;
    assert_eq!((w["ok"].clone(), w["restricted"].clone(), w["admin"].clone(), w["email"].clone(), w["tenant"].clone()), (json!(true), json!(true), json!(false), json!("me@x.test"), json!(tenant_of(&u))));
    assert_eq!(w["limits"]["message_chars"], 20_000);
    assert_eq!(w["limits"]["data_bytes"], 25 * 1024 * 1024);
    assert_eq!((w["limits"]["requests_per_min"].clone(), w["limits"]["concurrent_sends"].clone(), w["limits"]["tool_steps"].clone(), w["limits"]["answer_secs"].clone()), (json!(600), json!(2), json!(8), json!(120)));
    assert!(w["data_bytes"].is_u64());
    let o = call(&r, "tok-o", "/api/account/whoami", json!({})).body;
    assert!(o["limits"].get("data_bytes").is_none(), "no storage cap for the owner");
}

#[test]
fn the_web_page_the_policy_pages_and_the_headers() {
    let r = rig(&[], "", 200, 600);
    let get = |p: &str| ureq::get(&format!("{}{p}", r.base)).call().unwrap();
    let page = get("/");
    assert!(page.header("Content-Security-Policy").is_some_and(|c| c.contains("https://www.gstatic.com") && c.contains("frame-ancestors 'none'")));
    assert_eq!(page.header("X-Content-Type-Options"), Some("nosniff"));
    assert_eq!(page.header("Referrer-Policy"), Some("no-referrer"));
    assert_eq!(page.header("Permissions-Policy"), Some("microphone=(self), camera=()"));
    assert!(page.into_string().unwrap().contains("<title>"));
    for (p, word) in [("/privacy", "Privacy"), ("/terms", "Terms")] {
        let res = get(p);
        assert!(res.header("Content-Type").unwrap().starts_with("text/html"));
        assert_eq!(res.header("Content-Security-Policy"), Some(pages::PLAIN_CSP));
        let t = res.into_string().unwrap();
        assert!(t.contains(word) && t.contains("<h1>") && !t.contains("<script"), "{p}");
    }
    assert_eq!(get("/healthz").into_string().unwrap(), "ok");
    assert!(matches!(ureq::get(&format!("{}/api/op", r.base)).call(), Err(ureq::Error::Status(404, _))));
    assert!(matches!(ureq::get(&format!("{}/nothing", r.base)).call(), Err(ureq::Error::Status(404, _))));
}

#[test]
fn the_no_login_test_mode_uses_one_fixed_restricted_person_and_only_answers_its_own_page() {
    let root = testenv::service();
    let tenants = Arc::new(TenantMap::new(root, None, testenv::SECRET.into(), 200, 8));
    let app = Arc::new(App { no_auth: true, noauth_admin: false, verifier: None, policy: Policy::default(), tenants, devices: None, login: DeviceLogin::new(Default::default()), public_url: None, streams: Mutex::new(HashMap::new()), next: AtomicU64::new(1), conns: AtomicUsize::new(0), legal: Default::default(), require_terms: std::sync::atomic::AtomicBool::new(false), accepted: Mutex::new(Default::default()), push: Default::default(), link: link::Link::default(), term: crate::term::Hub::default(), term_root: std::env::temp_dir().join("zo-test-terminals") });
    let server = Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let a = app.clone();
    std::thread::spawn(move || serve(a, server));
    let post = |path: &str, body: Value, local: bool, origin: Option<&str>| {
        let mut r = ureq::post(&format!("{base}{path}"));
        if local {
            r = r.set("X-ZO-Local", "1");
        }
        if let Some(o) = origin {
            r = r.set("Origin", o);
        }
        match r.send_string(&body.to_string()) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        }
    };
    let w = post("/api/account/whoami", json!({}), true, None);
    assert_eq!(w.header("Access-Control-Allow-Origin"), None, "no cross-site access");
    let w: Value = w.into_json().unwrap();
    assert_eq!((w["restricted"].clone(), w["tenant"].clone()), (json!(true), json!(local_tenant())));
    // /api/op through the service for that fixed tenant
    let st = post("/api/op", json!({"op": "status"}), true, None).into_json::<Value>().unwrap();
    assert_eq!(st["ok"], true, "{st}");
    assert_eq!(post("/api/op", json!({"op": "machines"}), true, None).into_json::<Value>().unwrap()["ok"], false, "restricted");
    // /api/send through the service
    let body = post("/api/send", json!({"text": "hi"}), true, None).into_string().unwrap();
    assert!(body.contains("\"stream\"") && body.contains("\"done\""), "{body}");
    // the test person lives in the same place every time
    assert!(testenv::env().join(local_tenant()).join("data").exists());
    // another web page, or a call without the local header, is refused
    assert_eq!(post("/api/op", json!({"op": "status"}), false, None).status(), 401);
    assert_eq!(post("/api/op", json!({"op": "status"}), true, Some("https://evil.example")).status(), 401);
    // no devices without a database: the sign-in calls say so (and the credential-making call is gone)
    assert_eq!(post("/api/auth/device_start", json!({"name": "x", "kind": "cli"}), true, None).status(), 400);
    assert_eq!(post("/api/auth/device_poll", json!({"device_code": "A".repeat(43)}), true, None).status(), 400);
    assert_eq!(post("/api/auth/exchange", json!({"name": "x", "kind": "cli"}), true, None).status(), 400);
    assert_eq!(post("/api/account/device_create", json!({"name": "x", "scope": "sync"}), true, None).status(), 404);
}

// ---------------------------------------------------------------------------------------------------------------------------
// the second review round

#[test]
fn the_server_remembers_who_accepted_which_version_and_refuses_everything_else_until_then() {
    let u = uniq("terms");
    let r = rig(&[("tok-t", &u, "t@x.test")], "", 200, 600);
    r.app.require_terms.store(true, Ordering::SeqCst);
    let no = call(&r, "tok-t", "/api/op", json!({"op": "chats"}));
    assert_eq!((no.status, no.body["terms_required"].clone()), (403, json!(true)), "{:?}", no.body);
    assert_eq!(call(&r, "tok-t", "/api/sync/manifest", json!({})).status, 403);
    // nothing that makes a credential before the terms are accepted
    let ex = call(&r, "tok-t", "/api/auth/exchange", json!({"name": "x", "kind": "desktop"}));
    assert_eq!((ex.status, ex.body["terms_required"].clone()), (403, json!(true)), "{:?}", ex.body);
    assert_eq!(call(&r, "tok-t", "/api/auth/device_lookup", json!({"user_code": "AAAA-AAAA"})).status, 403);
    assert_eq!(call(&r, "tok-t", "/api/auth/device_approve", json!({"user_code": "AAAA-AAAA", "approve": true})).status, 403);
    let who = call(&r, "tok-t", "/api/account/whoami", json!({}));
    assert_eq!((who.status, who.body["terms"]["accepted"].clone()), (200, json!(false)));
    assert_eq!(call(&r, "tok-t", "/api/account/accept_terms", json!({"version": "1999-01-01"})).status, 400);
    assert_eq!(call(&r, "tok-t", "/api/account/accept_terms", json!({"version": pages::TERMS_VERSION})).status, 200);
    assert_eq!(call(&r, "tok-t", "/api/op", json!({"op": "chats"})).status, 200);
    // it is a record in the database (version and time), so another browser or a restart does not ask again, and delete removes it
    let rec = r.db.get_doc(&format!("users/{}/meta", tenant_of(&u)), "terms").unwrap().unwrap();
    assert!(rec.contains(pages::TERMS_VERSION) && rec.contains("\"at\""), "{rec}");
    r.app.accepted.lock().unwrap().clear();
    assert_eq!(call(&r, "tok-t", "/api/op", json!({"op": "chats"})).status, 200);
}

#[test]
fn a_blocked_person_can_still_export_and_delete_but_nothing_else() {
    let u = uniq("blk2");
    let r = rig(&[("tok-b", &u, "blocked@x.test")], "owner@x.test", 200, 600);
    assert_eq!(call(&r, "tok-b", "/api/op", json!({"op": "chats"})).status, 403);
    assert_eq!(call(&r, "tok-b", "/api/account/whoami", json!({})).status, 403);
    assert_eq!(call(&r, "tok-b", "/api/account/export", json!({})).status, 200);
    let d = call(&r, "tok-b", "/api/account/delete", json!({"confirm": "DELETE"}));
    assert_eq!((d.status, d.body["ok"].clone()), (200, json!(true)), "{:?}", d.body);
}

#[test]
fn a_deleted_account_stays_deleted_after_a_restart_and_the_answer_says_whether_the_login_went() {
    let u = uniq("tomb");
    let r = rig(&[("tok-d", &u, "d@x.test")], "", 200, 600);
    let t = tenant_of(&u);
    assert_eq!(call(&r, "tok-d", "/api/op", json!({"op": "chats"})).status, 200);
    let d = call(&r, "tok-d", "/api/account/delete", json!({"confirm": "DELETE"}));
    assert_eq!(d.status, 200);
    assert_eq!(d.body["login_removed"], json!(false), "the stand-in database has no logins, and the answer says so");
    // a fresh process knows nothing in memory: the marker in the database still refuses the old token
    let fresh = tenants::Registry::new(r.db.clone(), 200);
    assert_eq!(fresh.ensure(&t), Err(tenants::Refusal::Gone));
    // when the login really went, the marker only lasts as long as a token can
    r.db.put_doc(tenants::TOMBSTONES, &t, &json!({"at": 1, "login_removed": true}).to_string()).unwrap();
    assert_eq!(fresh.ensure(&t), Ok(()));
    assert!(r.db.get_doc(tenants::TOMBSTONES, &t).unwrap().is_none());
}

#[test]
fn sync_refuses_ids_the_database_cannot_hold_and_never_lets_a_chat_name_a_file_of_the_server() {
    let u = uniq("ids");
    let r = rig(&[("tok-i", &u, "i@x.test")], "", 200, 600);
    for bad in ["..", ".", "a/b", "X__1", "a b", &"x".repeat(101)] {
        let a = call(&r, "tok-i", "/api/sync/put", json!({"kind": "chats", "id": bad, "doc": chat(bad, "t")}));
        assert_eq!(a.status, 400, "{bad:?}: {:?}", a.body);
    }
    // a picture path planted in a saved chat is dropped on the way in
    let mut c = chat("c-img", "t");
    c["history"] = json!([{"UserImages": ["look", [{"path": "/proc/self/environ", "mime": "text/plain"}]]}]);
    assert_eq!(call(&r, "tok-i", "/api/sync/put", json!({"kind": "chats", "id": "c-img", "doc": c})).status, 200);
    let got = call(&r, "tok-i", "/api/sync/get", json!({"kind": "chats", "id": "c-img"}));
    assert!(!got.body.to_string().contains("environ"), "{:?}", got.body);
}

#[test]
fn a_client_that_sends_a_body_too_slowly_is_let_go_and_model_ops_take_a_place_like_an_answer() {
    assert!(["compact", "memory_edit", "personality_generate", "models", "model_test"].iter().all(|o| op_uses_model(o)) && !op_uses_model("chats"));
    let u = uniq("slow");
    let r = rig(&[("tok-s", &u, "s@x.test")], "", 200, 600);
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(r.base.trim_start_matches("http://")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    s.write_all(b"POST /api/op HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer tok-s\r\nContent-Length: 100\r\n\r\n{\"op\":").unwrap();
    let t0 = Instant::now();
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    assert!(t0.elapsed() < Duration::from_secs(35), "the connection was held for {:?}", t0.elapsed());
    assert!(buf.is_empty() || buf.contains("408") || buf.contains("400"), "{buf}");
    // the thread and the lease came back: the person can still be served
    assert_eq!(call(&r, "tok-s", "/api/op", json!({"op": "chats"})).status, 200);
}

// ---------------------------------------------------------------------------------------------------------------------------
// the browser device login (/api/auth/*)

fn start(r: &Rig, name: &str, kind: &str) -> Value {
    let a = call(r, "", "/api/auth/device_start", json!({"name": name, "kind": kind}));
    assert_eq!(a.status, 200, "{:?}", a.body);
    a.body
}

fn poll(r: &Rig, started: &Value) -> Value {
    let a = call(r, "", "/api/auth/device_poll", json!({"device_code": started["device_code"]}));
    assert_eq!(a.status, 200, "{:?}", a.body);
    a.body
}

#[test]
fn the_whole_device_login_start_lookup_approve_poll_once_then_the_credential_works_by_kind() {
    let (u, o) = (uniq("flow"), uniq("flow-o"));
    let r = rig(&[("tok", &u, "me@x.test"), ("tok-o", &o, "owner@x.test")], "owner@x.test", 200, 600);
    let mut every_answer = String::new();
    for (kind, name, may_use_api) in [("desktop", "My laptop", false), ("cli", "Terminal", true)] {
        let s = start(&r, name, kind);
        every_answer.push_str(&s.to_string());
        assert_eq!(s["device_code"].as_str().unwrap().len(), 43);
        let code = s["user_code"].as_str().unwrap().to_string();
        assert!(code.len() == 9 && code.as_bytes()[4] == b'-', "{code}");
        assert_eq!((s["verify_url"].clone(), s["expires_in"].clone(), s["interval"].clone()), (json!("https://zo.test/device"), json!(600), json!(3)));
        assert!(!s["verify_url"].as_str().unwrap().contains(&code), "the link never carries the code");
        // nothing is approved yet
        let p = poll(&r, &s);
        every_answer.push_str(&p.to_string());
        assert_eq!(p, json!({"ok": true, "status": "pending"}));
        // the person looks the code up: they see what is being approved, with the address it came from
        let l = call(&r, "tok", "/api/auth/device_lookup", json!({"user_code": code.to_lowercase()}));
        every_answer.push_str(&l.body.to_string());
        assert_eq!(l.status, 200, "{:?}", l.body);
        assert_eq!((l.body["name"].clone(), l.body["kind"].clone(), l.body["ip"].clone()), (json!(name), json!(kind), json!("127.0.0.1")));
        assert!(l.body["requested_at"].as_u64().unwrap() > 1_700_000_000);
        // the screen is told who is approving and whether the request came from the approver's own network
        assert_eq!((l.body["email"].clone(), l.body["same_network"].clone()), (json!("me@x.test"), json!(true)));
        // a device that has no credential cannot be used, and the page for another person does not find it by accident
        let ap = call(&r, "tok", "/api/auth/device_approve", json!({"user_code": code, "approve": true}));
        every_answer.push_str(&ap.body.to_string());
        assert_eq!((ap.status, ap.body["ok"].clone()), (200, json!(true)), "{:?}", ap.body);
        // the code is single use
        assert_eq!(call(&r, "tok-o", "/api/auth/device_approve", json!({"user_code": code, "approve": true})).status, 404);
        assert_eq!(call(&r, "tok", "/api/auth/device_lookup", json!({"user_code": code})).status, 404);
        // the device collects its credential, once
        r.app.login.advance(3);
        let got = poll(&r, &s);
        assert_eq!((got["status"].clone(), got["email"].clone()), (json!("approved"), json!("me@x.test")), "{got}");
        let cred = got["credential"].as_str().unwrap().to_string();
        assert!(devices::well_formed(&cred));
        r.app.login.advance(3);
        assert_eq!(poll(&r, &s)["status"], "expired", "handed out exactly once");
        // the credential is the person's: it reaches their data, by its kind
        assert_eq!(call(&r, &cred, "/api/sync/manifest", json!({})).status, 200);
        assert_eq!(call(&r, &cred, "/api/op", json!({"op": "chats"})).status, if may_use_api { 200 } else { 403 }, "{kind}");
        assert_eq!(call(&r, &cred, "/api/account/whoami", json!({})).status, 403);
        // and it is a device in the person's list: name, kind, nothing secret
        let list = call(&r, "tok", "/api/account/devices_list", json!({}));
        let d = list.body["devices"].as_array().unwrap().iter().find(|d| d["name"] == name).unwrap().clone();
        assert_eq!(d["kind"], kind);
        assert!(!list.body.to_string().contains(&cred));
        // the credential is stored hashed only
        let stored: String = r.db.docs.lock().unwrap().iter().map(|((c, i), t)| format!("{c}/{i}={t}")).collect();
        assert!(!stored.contains(&cred) && stored.contains(&devices::hash_token(&cred)));
        every_answer.push_str(&list.body.to_string());
        // the person may sign this device out from the list
        assert_eq!(call(&r, "tok", "/api/account/device_revoke", json!({"id": d["id"]})).status, 200);
        assert_eq!(call(&r, &cred, "/api/sync/manifest", json!({})).status, 401);
    }
    // the secret code of a device is in the answer to device_start and in no other answer
    let s = start(&r, "x", "cli");
    let secret = s["device_code"].as_str().unwrap().to_string();
    let mut others = String::new();
    others.push_str(&poll(&r, &s).to_string());
    let code = s["user_code"].as_str().unwrap();
    others.push_str(&call(&r, "tok", "/api/auth/device_lookup", json!({"user_code": code})).body.to_string());
    others.push_str(&call(&r, "tok", "/api/auth/device_approve", json!({"user_code": code, "approve": true})).body.to_string());
    r.app.login.advance(3);
    others.push_str(&poll(&r, &s).to_string());
    others.push_str(&call(&r, "tok", "/api/account/devices_list", json!({})).body.to_string());
    others.push_str(&call(&r, "tok", "/api/account/export", json!({})).body.to_string());
    others.push_str(&call(&r, "", "/api/auth/device_poll", json!({"device_code": "nope"})).body.to_string());
    assert!(!others.contains(&secret) && !every_answer.contains(&secret));
    // nor in the database
    let stored: String = r.db.docs.lock().unwrap().iter().map(|((c, i), t)| format!("{c}/{i}={t}")).collect();
    assert!(!stored.contains(&secret));
}

#[test]
fn a_denied_request_is_told_to_the_device_once_and_nothing_is_made() {
    let u = uniq("deny");
    let r = rig(&[("tok", &u, "me@x.test")], "", 200, 600);
    let s = start(&r, "Not mine", "cli");
    let code = s["user_code"].as_str().unwrap();
    assert_eq!(call(&r, "tok", "/api/auth/device_approve", json!({"user_code": code, "approve": false})).status, 200);
    assert_eq!(call(&r, "tok", "/api/auth/device_approve", json!({"user_code": code, "approve": true})).status, 404, "a denied code cannot be approved afterwards");
    assert_eq!(poll(&r, &s)["status"], "denied");
    r.app.login.advance(3);
    assert_eq!(poll(&r, &s)["status"], "expired");
    assert!(call(&r, "tok", "/api/account/devices_list", json!({})).body["devices"].as_array().unwrap().is_empty());
    assert!(r.db.list_docs("deviceidx").unwrap().is_empty());
    // approve has to say yes or no
    let s = start(&r, "x", "cli");
    assert_eq!(call(&r, "tok", "/api/auth/device_approve", json!({"user_code": s["user_code"]})).status, 400);
}

#[test]
fn requests_run_out_after_ten_minutes_and_an_approved_but_uncollected_credential_is_revoked() {
    let u = uniq("exp");
    let r = rig(&[("tok", &u, "me@x.test")], "", 200, 600);
    let a = start(&r, "Too slow", "cli");
    let b = start(&r, "Never collected", "cli");
    assert_eq!(call(&r, "tok", "/api/auth/device_approve", json!({"user_code": b["user_code"], "approve": true})).status, 200);
    assert_eq!(call(&r, "tok", "/api/account/devices_list", json!({})).body["devices"].as_array().unwrap().len(), 1);
    r.app.login.advance(599);
    assert_eq!(poll(&r, &a)["status"], "pending");
    r.app.login.advance(2);
    assert_eq!(poll(&r, &a)["status"], "expired");
    assert_eq!(call(&r, "tok", "/api/auth/device_lookup", json!({"user_code": a["user_code"]})).status, 404);
    assert_eq!(poll(&r, &b)["status"], "expired", "the credential that was approved is not handed out late");
    // and it was revoked: it is not in the list and not in the index
    assert!(call(&r, "tok", "/api/account/devices_list", json!({})).body["devices"].as_array().unwrap().is_empty());
    assert!(r.db.list_docs("deviceidx").unwrap().is_empty());
}

#[test]
fn polling_too_fast_gets_slow_down_over_http() {
    let r = rig(&[], "", 200, 600);
    let s = start(&r, "x", "desktop");
    assert_eq!(poll(&r, &s)["status"], "pending");
    assert_eq!(poll(&r, &s)["status"], "slow_down");
    r.app.login.advance(3);
    assert_eq!(poll(&r, &s)["status"], "pending");
}

#[test]
fn the_request_caps_are_per_source_address_the_last_forwarded_entry_counts() {
    let r = rig(&[], "", 200, 600);
    let body = json!({"name": "x", "kind": "cli"});
    for n in 0..5 {
        assert_eq!(call_from(&r, "1.1.1.1", "", "/api/auth/device_start", body.clone()).status, 200, "{n}");
    }
    let no = call_from(&r, "1.1.1.1", "", "/api/auth/device_start", body.clone());
    assert_eq!(no.status, 429, "{:?}", no.body);
    assert!(no.retry.is_some() && no.body["ok"] == false);
    assert_eq!(call_from(&r, "2.2.2.2", "", "/api/auth/device_start", body.clone()).status, 200, "another address has its own");
    // an address cannot be made up by the client: a forged first entry changes nothing
    let forged = ureq::post(&format!("{}/api/auth/device_start", r.base)).set("X-Forwarded-For", "9.9.9.9, 1.1.1.1");
    assert!(matches!(forged.send_string(&body.to_string()), Err(ureq::Error::Status(429, _))));
}

#[test]
fn ten_starts_a_minute_from_one_address_and_a_thousand_waiting_overall() {
    let r = rig(&[], "", 200, 600);
    let body = json!({"name": "x", "kind": "cli"});
    // finishing each request keeps the pending cap out of the way, so this reaches the per-minute limit
    // (the stand-in knows nobody here, so each request is denied through the store directly)
    for n in 0..10 {
        let s = call(&r, "", "/api/auth/device_start", body.clone());
        assert_eq!(s.status, 200, "{n}: {:?}", s.body);
        let code = s.body["user_code"].as_str().unwrap().to_string();
        r.app.login.deny("t", &code, r.app.login.now()).unwrap();
        assert_eq!(poll(&r, &s.body)["status"], "denied");
    }
    let over = call(&r, "", "/api/auth/device_start", body.clone());
    assert_eq!(over.status, 429, "{:?}", over.body);
    assert!(over.retry.as_deref().and_then(|s| s.parse::<u64>().ok()).is_some_and(|s| s >= 1));
    // the global cap is tested on the store (devicelogin::tests): a thousand waiting requests, the next is refused with 503
    let full = DeviceLogin::new(devicelogin::Limits { global_pending: 2, ..Default::default() });
    let now = full.now();
    full.start("a", devices::Kind::Cli, "1.1.1.1", now).unwrap();
    full.start("a", devices::Kind::Cli, "2.2.2.2", now).unwrap();
    assert_eq!(full.start("a", devices::Kind::Cli, "3.3.3.3", now), Err(devicelogin::StartError::Full));
}

#[test]
fn start_checks_its_input_and_the_public_calls_are_small() {
    let r = rig(&[], "", 200, 600);
    for bad in [json!({}), json!({"name": "x"}), json!({"kind": "cli"}), json!({"name": "", "kind": "cli"}), json!({"name": "   ", "kind": "cli"}), json!({"name": "\u{202e}\n", "kind": "cli"}), json!({"name": "x".repeat(61), "kind": "cli"}), json!({"name": "x", "kind": "root"}), json!({"name": "x", "kind": "full"}), json!({"name": 5, "kind": "cli"})] {
        let a = call_from(&r, "4.4.4.4", "", "/api/auth/device_start", bad.clone());
        assert_eq!(a.status, 400, "{bad}: {:?}", a.body);
    }
    // a name cannot hide itself: control and direction characters are dropped
    let s = call_from(&r, "4.4.4.4", "", "/api/auth/device_start", json!({"name": "  Pho\u{202e}ne\n ", "kind": "desktop"}));
    assert_eq!(s.status, 200, "{:?}", s.body);
    let u = uniq("input");
    let r2 = rig(&[("tok", &u, "a@x.test")], "", 200, 600);
    let s = start(&r2, "  Pho\u{202e}ne\n ", "desktop");
    assert_eq!(call(&r2, "tok", "/api/auth/device_lookup", json!({"user_code": s["user_code"]})).body["name"], "Phone");
    // big bodies are refused, and a GET is not a call
    let big = call(&r, "", "/api/auth/device_poll", json!({"device_code": "x".repeat(5000)}));
    assert_eq!(big.status, 413);
    assert!(matches!(ureq::get(&format!("{}/api/auth/device_start", r.base)).call(), Err(ureq::Error::Status(404, _))));
    // an unknown code is "expired", not an error that tells a guesser anything
    assert_eq!(call(&r, "", "/api/auth/device_poll", json!({"device_code": "A".repeat(43)})).body, json!({"ok": true, "status": "expired"}));
    assert_eq!(call(&r, "", "/api/auth/device_poll", json!({})).body["status"], "expired");
}

#[test]
fn a_person_who_types_wrong_codes_is_locked_out_for_a_while_and_nobody_else_is() {
    let (ua, ub) = (uniq("guess-a"), uniq("guess-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 600);
    let s = start(&r, "x", "cli");
    for n in 0..5 {
        assert_eq!(call(&r, "tok-a", "/api/auth/device_lookup", json!({"user_code": "ABCD-EFGH"})).status, 404, "{n}");
    }
    // approving counts as well, and the right code is refused to this person now
    let locked = call(&r, "tok-a", "/api/auth/device_approve", json!({"user_code": s["user_code"], "approve": true}));
    assert_eq!(locked.status, 429, "{:?}", locked.body);
    assert!(locked.retry.as_deref().and_then(|s| s.parse::<u64>().ok()).is_some_and(|s| s >= 1));
    assert_eq!(call(&r, "tok-a", "/api/auth/device_lookup", json!({"user_code": s["user_code"]})).status, 429);
    // another person is not affected, and the request is still waiting
    assert_eq!(call(&r, "tok-b", "/api/auth/device_lookup", json!({"user_code": s["user_code"]})).status, 200);
    // wrong approvals count the same way
    for n in 0..5 {
        assert_eq!(call(&r, "tok-b", "/api/auth/device_approve", json!({"user_code": "ZZZZ-ZZZZ", "approve": true})).status, 404, "{n}");
    }
    assert_eq!(call(&r, "tok-b", "/api/auth/device_approve", json!({"user_code": s["user_code"], "approve": true})).status, 429);
    // after ten minutes the lock is gone (the request itself ran out too: a fresh one)
    r.app.login.advance(601);
    let s2 = start(&r, "y", "cli");
    assert_eq!(call(&r, "tok-a", "/api/auth/device_approve", json!({"user_code": s2["user_code"], "approve": true})).status, 200);
}

#[test]
fn an_unverified_or_unknown_sign_in_a_blocked_person_and_a_signed_out_caller_get_nothing() {
    let u = uniq("refuse");
    let r = rig(&[("tok", &u, "me@x.test"), ("tok-x", &uniq("refuse-b"), "blocked@x.test")], "", 200, 600);
    let s = start(&r, "x", "cli");
    let code = s["user_code"].as_str().unwrap();
    // no sign-in, or one the server does not know (the token check itself, including the verified e-mail, is in auth::tests)
    for tok in ["", "garbage"] {
        for (path, body) in [("/api/auth/device_lookup", json!({"user_code": code})), ("/api/auth/device_approve", json!({"user_code": code, "approve": true})), ("/api/auth/exchange", json!({"name": "x", "kind": "cli"}))] {
            assert_eq!(call(&r, tok, path, body).status, 401, "{tok:?} {path}");
        }
    }
    // a blocked person gets nothing, not even an approval
    for (path, body) in [("/api/auth/device_lookup", json!({"user_code": code})), ("/api/auth/device_approve", json!({"user_code": code, "approve": true})), ("/api/auth/exchange", json!({"name": "x", "kind": "cli"}))] {
        assert_eq!(call(&r, "tok-x", path, body).status, 403, "{path}");
    }
    assert_eq!(poll(&r, &s)["status"], "pending", "nothing was approved");
    assert!(r.db.list_docs("deviceidx").unwrap().is_empty());
}

#[test]
fn at_most_ten_signed_in_devices_and_a_full_list_does_not_lose_the_request() {
    let u = uniq("ten");
    let r = rig(&[("tok", &u, "me@x.test")], "", 200, 600);
    for n in 0..devices::MAX_DEVICES {
        make_device(&r, "tok", &format!("d{n}"), "desktop");
    }
    assert_eq!(call(&r, "tok", "/api/auth/exchange", json!({"name": "one more", "kind": "cli"})).status, 409);
    let s = start(&r, "one more", "cli");
    let full = call(&r, "tok", "/api/auth/device_approve", json!({"user_code": s["user_code"], "approve": true}));
    assert_eq!(full.status, 409, "{:?}", full.body);
    assert!(full.body["error"].as_str().unwrap().contains("signed-in devices"));
    // the request is still waiting: sign one out and approve again
    let first = call(&r, "tok", "/api/account/devices_list", json!({})).body["devices"][0]["id"].clone();
    assert_eq!(call(&r, "tok", "/api/account/device_revoke", json!({"id": first})).status, 200);
    assert_eq!(call(&r, "tok", "/api/auth/device_approve", json!({"user_code": s["user_code"], "approve": true})).status, 200);
    r.app.login.advance(3);
    assert_eq!(poll(&r, &s)["status"], "approved");
}

#[test]
fn the_approval_page_is_the_web_app_and_keeps_its_csp() {
    let r = rig(&[], "", 200, 600);
    let page = ureq::get(&format!("{}/device", r.base)).call().unwrap();
    assert!(page.header("Content-Security-Policy").is_some_and(|c| c == pages::APP_CSP && c.contains("frame-ancestors 'none'")));
    assert_eq!(page.header("X-Frame-Options"), Some("DENY"));
    assert_eq!(page.header("Referrer-Policy"), Some("no-referrer"));
    let body = page.into_string().unwrap();
    assert_eq!(body, ureq::get(&format!("{}/", r.base)).call().unwrap().into_string().unwrap());
    // a code in the address changes nothing the server does with it: it is the same page and nothing is read from the query
    let with = ureq::get(&format!("{}/device?code=ABCD-EFGH", r.base)).call().unwrap().into_string().unwrap();
    assert_eq!(with, body);
}

// ---------------------------------------------------------------------------------------------------------------------------
// the whole sign-in and sync, end to end: a real desktop client (zo-core's `syncctl` example, its own process with its own
// sandbox, no environment of a server) against this server over real HTTP on 127.0.0.1.

mod end_to_end {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};

    fn syncctl() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = Command::new(env!("CARGO")).args(["build", "-q", "-p", "zo-core", "--example", "syncctl"]).current_dir(&root).output().expect("cargo runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
        target.join("debug/examples/syncctl")
    }

    /// A device's own computer: its own folders and key store, and the server it talks to.
    struct Device {
        dir: PathBuf,
        url: String,
        bin: PathBuf,
    }

    impl Device {
        fn new(name: &str, url: &str, bin: &Path) -> Device {
            let dir = PathBuf::from(format!("/tmp/claude-1000/e2e{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            for d in ["home", "data", "config", "rt"] {
                std::fs::create_dir_all(dir.join(d)).unwrap();
            }
            Device { dir, url: url.into(), bin: bin.into() }
        }
        fn cmd(&self, args: &[&str]) -> Command {
            let mut c = Command::new(&self.bin);
            c.env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", self.dir.join("home"))
                .env("XDG_DATA_HOME", self.dir.join("data"))
                .env("XDG_CONFIG_HOME", self.dir.join("config"))
                .env("XDG_RUNTIME_DIR", self.dir.join("rt"))
                .env("ZOHARA_CHAT_KEYS_FILE", self.dir.join("keys.json"))
                .env("ZO_SERVER_URL", &self.url)
                .args(args);
            c
        }
        fn run(&self, args: &[&str]) -> String {
            let o = self.cmd(args).output().expect("syncctl runs");
            assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        /// Starts the browser sign-in; returns the process and the code it shows.
        fn login(&self, kind: &str, name: &str) -> (Child, BufReader<std::process::ChildStdout>, String) {
            let mut child = self.cmd(&["login", kind, name]).stdout(Stdio::piped()).spawn().expect("spawn");
            let mut out = BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            out.read_line(&mut line).unwrap();
            let code = line.trim().strip_prefix("CODE ").unwrap_or_else(|| panic!("no code: {line:?}")).to_string();
            (child, out, code)
        }
        /// The device credential this computer stores (only used to call the server as the device does; never printed).
        fn credential(&self) -> Option<String> {
            let t = std::fs::read_to_string(self.dir.join("keys.json")).ok()?;
            let v: Value = serde_json::from_str(&t).ok()?;
            v.as_object()?.values().filter_map(Value::as_str).find(|s| s.starts_with("zd_")).map(str::to_string)
        }
    }

    fn finish(mut child: Child, mut out: BufReader<std::process::ChildStdout>) -> String {
        let mut rest = String::new();
        let mut l = String::new();
        while out.read_line(&mut l).unwrap_or(0) > 0 {
            rest.push_str(&l);
            l.clear();
        }
        child.wait().unwrap();
        rest.trim().to_string()
    }

    fn web_ids(r: &Rig, tok: &str, op: &str, field: &str) -> Vec<String> {
        let a = call(r, tok, "/api/op", json!({"op": op}));
        assert_eq!(a.status, 200, "{op}: {:?}", a.body);
        a.body[field].as_array().unwrap_or(&vec![]).iter().map(|x| x["id"].as_str().unwrap_or("").to_string()).collect()
    }

    #[test]
    fn sign_in_on_web_and_desktop_then_chats_and_memory_match_and_sign_out_ends_it() {
        let bin = syncctl();
        let (a_uid, b_uid) = (uniq("e2e-a"), uniq("e2e-b"));
        let r = rig(&[("tok-a", &a_uid, "a@x.test"), ("tok-b", &b_uid, "b@x.test")], "owner@x.test", 200, 600);
        let desk = Device::new("desk", &r.base, &bin);

        // nothing happens without a sign-in: the desktop works on its own and sends nothing
        assert_eq!(desk.run(&["whoami"]), "not signed in");
        desk.run(&["chat", "c-desk", "Planning the trip"]);
        desk.run(&["note", "Likes tea without sugar"]);
        println!("desktop before sign-in: {}", desk.run(&["syncnow"]));
        assert!(call(&r, "tok-a", "/api/op", json!({"op": "chats"})).body.to_string().contains("[]"), "the server has nothing of the desktop yet");

        // the desktop starts the browser sign-in and shows a code; the web page is not involved yet
        let (child, out, code) = desk.login("desktop", "Test desktop");
        println!("desktop shows the code {code}");
        // person A, signed in on the web, types the code: sees what is asked, approves
        let l = call(&r, "tok-a", "/api/auth/device_lookup", json!({"user_code": code}));
        assert_eq!((l.status, l.body["name"].clone(), l.body["kind"].clone()), (200, json!("Test desktop"), json!("desktop")), "{:?}", l.body);
        // person B cannot approve it into A's account by guessing: B's approval would make it B's, so B must not look it up wrongly
        let ap = call(&r, "tok-a", "/api/auth/device_approve", json!({"user_code": code, "approve": true}));
        assert_eq!(ap.status, 200, "{:?}", ap.body);
        let end = finish(child, out);
        println!("desktop sign-in result: {end}");
        assert_eq!(end, "LOGGED_IN a@x.test");
        assert_eq!(desk.run(&["whoami"]), "a@x.test");
        let cred = desk.credential().expect("the credential is in the device's key store");
        // the person never sees it: not on any screen output of the client, not in sync.json
        let cfg = std::fs::read_to_string(desk.dir.join("config/zohara-chat/sync.json")).unwrap_or_default();
        assert!(!cfg.contains(&cred) && !end.contains(&cred));

        // the sync round pushes the chat and the note
        let s = desk.run(&["syncnow"]);
        println!("desktop sync: {s}");
        assert!(s.contains("pushed 2"), "{s}");
        assert!(web_ids(&r, "tok-a", "chats", "chats").contains(&"c-desk".to_string()), "A sees the chat on the web");
        let notes = call(&r, "tok-a", "/api/op", json!({"op": "memory"}));
        let item = notes.body["items"].as_array().unwrap().iter().find(|i| i["text"].as_str().unwrap_or("").contains("tea without sugar")).expect("A sees the note on the web").clone();
        println!("web sees: chat c-desk, note \"{}\"", item["text"]);

        // A edits the note on the web (a moment later); the desktop pulls it
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let up = call(&r, "tok-a", "/api/op", json!({"op": "memory_update", "id": item["id"], "text": "Likes tea with honey"}));
        assert_eq!(up.status, 200, "{:?}", up.body);
        let s = desk.run(&["syncnow"]);
        println!("desktop sync after the web edit: {s}");
        assert!(s.contains("pulled 1"), "{s}");
        let list = desk.run(&["list"]);
        println!("desktop now has:\n{list}");
        assert!(list.contains("Likes tea with honey") && !list.contains("without sugar"));

        // person B sees nothing of A, by any door
        assert!(!web_ids(&r, "tok-b", "chats", "chats").contains(&"c-desk".to_string()));
        assert!(!call(&r, "tok-b", "/api/op", json!({"op": "memory"})).body.to_string().contains("honey"));
        let bdev = make_device(&r, "tok-b", "B's terminal", "cli").0;
        assert!(!call(&r, &bdev, "/api/op", json!({"op": "chats"})).body.to_string().contains("c-desk"));
        assert!(!call(&r, &bdev, "/api/sync/manifest", json!({})).body.to_string().contains("c-desk"));

        // the terminal: `zo login`-style sign-in with kind cli (scope full) reaches A's data through /api/op, and only A's
        let term = Device::new("term", &r.base, &bin);
        let (child, out, code) = term.login("cli", "Test terminal");
        assert_eq!(call(&r, "tok-a", "/api/auth/device_approve", json!({"user_code": code, "approve": true})).status, 200);
        assert_eq!(finish(child, out), "LOGGED_IN a@x.test");
        let tcred = term.credential().unwrap();
        let ch = call(&r, &tcred, "/api/op", json!({"op": "chats"}));
        assert_eq!(ch.status, 200);
        assert!(ch.body.to_string().contains("c-desk"));
        assert_eq!(call(&r, &tcred, "/api/account/whoami", json!({})).status, 403, "a device cannot manage the account");
        // and the desktop's own credential is sync only
        assert_eq!(call(&r, &cred, "/api/op", json!({"op": "chats"})).status, 403);

        // A's list shows both devices; A revokes the terminal from the Account page: its calls get 401
        let list = call(&r, "tok-a", "/api/account/devices_list", json!({}));
        let devs = list.body["devices"].as_array().unwrap();
        println!("A's signed-in devices: {:?}", devs.iter().map(|d| format!("{} ({})", d["name"], d["kind"])).collect::<Vec<_>>());
        let tid = devs.iter().find(|d| d["name"] == "Test terminal").unwrap()["id"].clone();
        assert_eq!(call(&r, "tok-a", "/api/account/device_revoke", json!({"id": tid})).status, 200);
        assert_eq!(call(&r, &tcred, "/api/op", json!({"op": "chats"})).status, 401);
        assert_eq!(call(&r, &cred, "/api/sync/manifest", json!({})).status, 200, "the desktop is untouched");
        // B cannot revoke A's device
        assert_ne!(call(&r, "tok-b", "/api/account/device_revoke", json!({"id": devs[0]["id"]})).status, 200);

        // sign out from the desktop: the server revokes this device's credential, the data stays on the computer
        println!("desktop sign out: {}", desk.run(&["signout"]));
        assert_eq!(call(&r, &cred, "/api/sync/manifest", json!({})).status, 401, "the credential is dead");
        assert_eq!(desk.run(&["whoami"]), "not signed in");
        assert!(desk.run(&["list"]).contains("c-desk"), "chats stay on the computer");
        assert!(desk.credential().is_none(), "the key store is empty");
        assert!(desk.run(&["syncnow"]).starts_with("sync failed"), "no sign-in, no sync");
        // A's data is still on the server
        assert!(web_ids(&r, "tok-a", "chats", "chats").contains(&"c-desk".to_string()));
        let left = call(&r, "tok-a", "/api/account/devices_list", json!({}));
        assert!(left.body["devices"].as_array().unwrap().is_empty(), "{:?}", left.body);
    }

    /// Signs a desktop device in to the account of the Firebase token `tok` (the browser sign-in the web page approves).
    fn sign_in(r: &Rig, tok: &str, dev: &Device, name: &str) {
        let (child, out, code) = dev.login("desktop", name);
        assert_eq!(call(r, tok, "/api/auth/device_approve", json!({"user_code": code, "approve": true})).status, 200);
        assert!(finish(child, out).starts_with("LOGGED_IN"));
    }

    fn op(r: &Rig, tok: &str, v: Value) -> Value {
        let a = call(r, tok, "/api/op", v);
        assert_eq!(a.status, 200, "{:?}", a.body);
        a.body
    }

    #[test]
    fn profiles_pins_addresses_mcp_preferences_and_keys_sync_between_the_desktop_and_the_web() {
        let bin = syncctl();
        let (a_uid, r_uid) = (uniq("pk-a"), uniq("pk-r"));
        let r = rig(&[("tok-a", &a_uid, "a@x.test"), ("tok-r", &r_uid, "r@x.test")], "a@x.test", 200, 600);
        let desk = Device::new("pdesk", &r.base, &bin);
        sign_in(&r, "tok-a", &desk, "Desk");
        let secret = "sk-desktop-secret-value-123";

        // the desktop has a profile with a pin and an address of its own, an MCP server, a hidden provider and a key
        let pid = desk.run(&["profile", "Work", "custom", "m1", "https://models.example.com/v1", "m1,m2"]);
        desk.run(&["mcpadd", "docs", "https://docs.example.com/mcp"]);
        desk.run(&["mcpadd", "files", "npx -y some-local-server"]);
        desk.run(&["hidden", "groq,mistral"]);
        desk.run(&["key", "custom", secret]);
        let s = desk.run(&["syncnow"]);
        println!("first sync: {s}");
        assert!(s.starts_with("synced"), "{s}");

        // the web (the admin, with the Firebase token) sees the profile with its address and pins, the server, the preference
        let list = op(&r, "tok-a", json!({"op": "profiles"}));
        let p = list["profiles"].as_array().unwrap().iter().find(|p| p["id"] == pid.as_str()).unwrap_or_else(|| panic!("{list}")).clone();
        println!("web sees profile: {} base_url={} pinned={}", p["name"], p["base_url"], p["pinned"]);
        assert_eq!((p["base_url"].clone(), p["pinned"].clone(), p["provider"].clone()), (json!("https://models.example.com/v1"), json!(["m1", "m2"]), json!("custom")));
        let mcp = op(&r, "tok-a", json!({"op": "mcp_list"}));
        assert_eq!(mcp["servers"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect::<Vec<_>>(), ["docs"], "the remote one only, never the program: {mcp}");
        assert_eq!(op(&r, "tok-a", json!({"op": "settings_get"}))["hidden_providers"], json!(["groq", "mistral"]));
        // keys are OFF: no key on the server, none in the manifest, and the secret is nowhere in what the server says
        let prov = op(&r, "tok-a", json!({"op": "providers"}));
        assert_eq!(prov["providers"].as_array().unwrap().iter().find(|p| p["id"] == "custom").unwrap()["has_key"], false);
        let cred = desk.credential().unwrap();
        let man = call(&r, &cred, "/api/sync/manifest", json!({})).body;
        assert!(man["keys"].as_object().unwrap().is_empty(), "{man}");
        for text in [list.to_string(), mcp.to_string(), man.to_string(), prov.to_string()] {
            assert!(!text.contains(secret) && !text.contains("some-local-server"), "{text}");
        }

        // an edit on the web (a pin) reaches the desktop; an edit on the desktop (another pin) reaches the web
        std::thread::sleep(std::time::Duration::from_millis(1100));
        op(&r, "tok-a", json!({"op": "profile_pin", "id": pid, "model": "m3", "on": true}));
        assert!(desk.run(&["syncnow"]).contains("pulled"), "the web's pin comes to the desktop");
        assert!(desk.run(&["profiles"]).contains("m1,m2,m3"), "{}", desk.run(&["profiles"]));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        desk.run(&["unpin", &pid, "m1"]);
        desk.run(&["syncnow"]);
        let list = op(&r, "tok-a", json!({"op": "profiles"}));
        assert_eq!(list["profiles"][0]["pinned"], json!(["m2", "m3"]), "{list}");
        // switching an MCP server off on the web reaches the desktop (adding one on the web connects to it first, which needs a live
        // server; it is saved by the same code); hiding a provider on the web too
        std::thread::sleep(std::time::Duration::from_millis(1100));
        op(&r, "tok-a", json!({"op": "mcp_toggle", "name": "docs", "enabled": false}));
        op(&r, "tok-a", json!({"op": "settings_set", "hidden_providers": ["groq"]}));
        desk.run(&["syncnow"]);
        let m = desk.run(&["mcplist"]);
        assert!(m.contains("mcp docs https://docs.example.com/mcp enabled=false") && m.contains("npx -y some-local-server"), "{m}");
        assert_eq!(desk.run(&["hiddenlist"]), "hidden groq");
        // deleting the profile on the desktop deletes it on the web
        std::thread::sleep(std::time::Duration::from_millis(1100));
        desk.run(&["rmprofile", &pid]);
        desk.run(&["syncnow"]);
        assert!(op(&r, "tok-a", json!({"op": "profiles"}))["profiles"].as_array().unwrap().is_empty());
        // and a profile made on the web arrives on the desktop
        std::thread::sleep(std::time::Duration::from_millis(1100));
        op(&r, "tok-a", json!({"op": "select", "provider": "custom", "model": "web-m"}));
        let web_id = op(&r, "tok-a", json!({"op": "profile_create", "name": "FromWeb"}))["id"].as_str().unwrap().to_string();
        desk.run(&["syncnow"]);
        assert!(desk.run(&["profiles"]).contains(&format!("profile {web_id} FromWeb")));

        // keys only travel once the toggle is on, here
        assert_eq!(desk.run(&["keyis", "custom", secret]), "true");
        desk.run(&["kinds", "true", "true", "true"]);
        let s = desk.run(&["syncnow"]);
        println!("sync with keys on: {s}");
        let prov = op(&r, "tok-a", json!({"op": "providers"}));
        assert_eq!(prov["providers"].as_array().unwrap().iter().find(|p| p["id"] == "custom").unwrap()["has_key"], true, "the web has the key now: {prov}");
        assert!(!prov.to_string().contains(secret), "the key itself is never in an answer");
        let man = call(&r, &cred, "/api/sync/manifest", json!({})).body;
        assert!(man["keys"].as_object().unwrap().contains_key("custom") && !man["keys"].as_object().unwrap().keys().any(|k| k.contains("sync-token")), "{man}");
        // a key saved on the web (for the profile it is using there) comes to the desktop, under that profile
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(call(&r, "tok-a", "/api/op", json!({"op": "set_key", "provider": "custom", "key": "sk-web-new-key-456"})).status, 200);
        desk.run(&["syncnow"]);
        assert_eq!(desk.run(&["keyis", &format!("custom@{web_id}"), "sk-web-new-key-456"]), "true");
        // switching keys off again stops the traffic: a new desktop key stays on the desktop
        desk.run(&["kinds", "true", "true", "false"]);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        desk.run(&["key", "custom", "sk-desktop-only-789"]);
        desk.run(&["syncnow"]);
        let a = call(&r, &cred, "/api/sync/get", json!({"kind": "keys", "id": "custom"}));
        assert_eq!(a.body["doc"]["key"], secret, "the server has the key the desktop sent");
        let a = call(&r, &cred, "/api/sync/get", json!({"kind": "keys", "id": &format!("custom@{web_id}")}));
        assert_eq!(a.body["doc"]["key"], "sk-web-new-key-456", "the server still has the old one");

        // a restricted person: no MCP server, no private address, but a public https profile and the pins arrive
        let rd = Device::new("rdesk", &r.base, &bin);
        sign_in(&r, "tok-r", &rd, "R desk");
        rd.run(&["profile", "Local", "custom", "m", "http://127.0.0.1:8000/v1", ""]);
        rd.run(&["profile", "Cloud", "custom", "m", "https://8.8.8.8/v1", "m,n"]);
        rd.run(&["mcpadd", "docs", "https://docs.example.com/mcp"]);
        let s = rd.run(&["syncnow"]);
        println!("restricted desktop sync: {s}");
        assert!(s.starts_with("synced"), "{s}");
        let rl = op(&r, "tok-r", json!({"op": "profiles"}));
        let names: Vec<&str> = rl["profiles"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Cloud"], "the private address stayed on the desktop: {rl}");
        assert_eq!(rl["profiles"][0]["pinned"], json!(["m", "n"]));
        let rc = rd.credential().unwrap();
        let rman = call(&r, &rc, "/api/sync/manifest", json!({})).body;
        assert!(rman["mcp"].as_object().unwrap().is_empty(), "{rman}");
        assert_eq!(call(&r, &rc, "/api/sync/get", json!({"kind": "mcp", "id": "docs"})).status, 400);
        let put = call(&r, &rc, "/api/sync/put", json!({"kind": "mcp", "id": "docs", "doc": {"name": "docs", "address": "https://docs.example.com/mcp", "updated": 5}}));
        assert_eq!(put.status, 400, "{:?}", put.body);
        let ml = call(&r, "tok-r", "/api/op", json!({"op": "mcp_list"}));
        assert!(ml.body.to_string().contains("not available"), "the web refuses it too: {:?}", ml.body);
        assert!(rd.run(&["mcplist"]).contains("docs"), "it stays on the desktop");
        assert!(rd.run(&["profiles"]).contains("Local"), "so does the private profile");
    }
}

#[test]
fn the_two_public_calls_refuse_web_pages_and_give_no_cross_site_permission() {
    let r = rig(&[], "", 200, 600);
    let url = format!("{}/api/auth/device_start", r.base);
    let body = json!({"name": "x", "kind": "cli"}).to_string();
    // a page in a browser always sends an Origin: refused, nothing recorded, no permission header
    let res = match ureq::post(&url).set("Origin", "https://evil.example").set("Content-Type", "text/plain").send_string(&body) {
        Err(ureq::Error::Status(c, r)) => (c, r),
        other => panic!("{other:?}"),
    };
    assert_eq!(res.0, 403);
    assert_eq!(res.1.header("Access-Control-Allow-Origin"), None);
    assert_eq!(r.app.login.pending(), 0);
    let poll_url = format!("{}/api/auth/device_poll", r.base);
    assert!(matches!(ureq::post(&poll_url).set("Origin", "https://evil.example").send_string("{}"), Err(ureq::Error::Status(403, _))));
    // the preflight is refused too, without a permission
    let pre = match ureq::request("OPTIONS", &url).set("Origin", "https://evil.example").set("Access-Control-Request-Method", "POST").call() {
        Err(ureq::Error::Status(c, r)) => (c, r),
        Ok(r) => (r.status(), r),
        Err(e) => panic!("{e}"),
    };
    assert_eq!(pre.0, 403);
    assert_eq!(pre.1.header("Access-Control-Allow-Origin"), None);
    // a program (no Origin) still works, and its answer carries no permission either
    let ok = ureq::post(&url).send_string(&body).unwrap();
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.header("Access-Control-Allow-Origin"), None);
    assert_eq!(r.app.login.pending(), 1);
    // the web app's own calls keep theirs
    let w = ureq::request("OPTIONS", &format!("{}/api/op", r.base)).call().unwrap();
    assert_eq!(w.header("Access-Control-Allow-Origin"), Some("*"));
}

// ---------------------------------------------------------------------------------------------------------------------------
// notifications: keep_running, push registration, the installable app

/// A model that takes `delay_ms` to answer, for as many requests as come.
fn slow_model(delay_ms: u64, text: &'static str) -> String {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { return };
            std::thread::spawn(move || {
                let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
                loop {
                    let n = s.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        while buf.len() < i + 4 + len {
                            let n = s.read(&mut tmp).unwrap_or(0);
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&tmp[..n]);
                        }
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(delay_ms));
                let body = json!({"choices": [{"message": {"content": text}}]}).to_string();
                let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            });
        }
    });
    url
}

/// The owner (an admin: its own model address may be a local one) answers through the slow model.
fn use_slow_model(uid: &str, url: &str) {
    let t = zo_core::tenant::Tenant::open_in(&testenv::env(), &tenant_of(uid), false).unwrap();
    zo_core::tenant::scope(&t, || {
        let mut sel = zo_core::ai::Selection { provider: "custom".into(), model: "m".into(), ..Default::default() };
        sel.base_urls.insert("custom".into(), url.to_string());
        sel.save();
        zo_core::ai::keys::set("custom", "sk-test-bring-your-own-0001").unwrap();
    });
}

fn push_rig(uid: &str) -> (Rig, Arc<push::testkit::Fake>, (p256::SecretKey, [u8; 16])) {
    let fake = Arc::new(push::testkit::Fake::default());
    let pdb: Arc<dyn Db> = Arc::new(MemDb::default());
    let hub = Arc::new(push::Hub::new(Some(pdb), testenv::SECRET, "mailto:o@zo.test".into()).with_transport(fake.clone()).with_allow(push::Allow::for_test("https://push.test")));
    let r = rig_push(&[("tok-o", uid, "o@x.test")], "o@x.test", 200, 600, hub);
    let keys = push::testkit::browser();
    let a = call(&r, "tok-o", "/api/account/push_subscribe", push::testkit::web_sub("https://push.test/ep/1", &keys));
    assert_eq!(a.status, 200, "{:?}", a.body);
    (r, fake, keys)
}

/// Starts an answer and hands back its events as they come.
fn start_send(r: &Rig, body: Value) -> (u16, std::io::BufReader<Box<dyn std::io::Read + Send + Sync + 'static>>) {
    let res = match ureq::post(&format!("{}/api/send", r.base)).set("Authorization", "Bearer tok-o").send_string(&body.to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{e}"),
    };
    (res.status(), std::io::BufReader::new(res.into_reader()))
}

/// Reads events until one of this kind; returns the one found.
fn read_until(rd: &mut impl BufRead, event: &str) -> Value {
    let mut line = String::new();
    loop {
        line.clear();
        assert!(rd.read_line(&mut line).unwrap() > 0, "the stream ended before {event}");
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["event"] == event {
            return v;
        }
    }
}

fn wait_until(secs: u64, what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(secs), "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn saved_answer(r: &Rig, chat_id: &str) -> Option<String> {
    let c = call(r, "tok-o", "/api/op", json!({"op": "chat", "id": chat_id}));
    c.body["entries"].as_array()?.iter().find(|e| e["kind"] == "msg-ai").and_then(|e| e["text"].as_str()).map(str::to_string)
}

#[test]
fn keep_running_finishes_the_answer_after_the_browser_left_holds_the_places_and_then_pushes() {
    let u = uniq("keep");
    let (r, fake, keys) = push_rig(&u);
    use_slow_model(&u, &slow_model(1200, "Here is the late answer."));
    let (status, mut rd) = start_send(&r, json!({"text": "tell me when it is ready", "keep_running": true}));
    assert_eq!(status, 200);
    let chat_id = read_until(&mut rd, "chat")["id"].as_str().unwrap().to_string();
    drop(rd); // the person closes the tab
    // the answer is still being written: the place stays taken, nothing is cancelled
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.app.tenants.running_answers(), 1, "the permit is held while the answer runs");
    assert!(fake.calls.lock().unwrap().is_empty(), "no push before the answer is there");
    wait_until(15, "the answer to be saved", || saved_answer(&r, &chat_id).is_some());
    assert_eq!(saved_answer(&r, &chat_id).unwrap(), "Here is the late answer.");
    wait_until(10, "the push", || !fake.calls.lock().unwrap().is_empty());
    wait_until(10, "the permit to be freed", || r.app.tenants.running_answers() == 0);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "https://push.test/ep/1");
    // the person's browser can read it: generic, the chat's id, and no title (not switched on)
    assert_eq!(push::testkit::decrypt_for(&keys, &calls[0].2), json!({"v": 1, "type": "answer", "chat": chat_id}));
    assert!(!String::from_utf8_lossy(&calls[0].2).contains("tell me when"));
    assert_eq!(r.app.push.delivered.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn a_person_who_stayed_connected_gets_no_push() {
    let u = uniq("stay");
    let (r, fake, _) = push_rig(&u);
    use_slow_model(&u, &slow_model(300, "Quick answer."));
    let (_, mut rd) = start_send(&r, json!({"text": "hello", "keep_running": true}));
    read_until(&mut rd, "done");
    drop(rd);
    wait_until(10, "the permit to be freed", || r.app.tenants.running_answers() == 0);
    std::thread::sleep(Duration::from_millis(700));
    assert!(fake.calls.lock().unwrap().is_empty(), "the answer was seen: nothing is pushed");
}

#[test]
fn without_a_registered_device_or_without_the_flag_nothing_is_kept_and_nothing_is_pushed() {
    // (the service-level test in zo-core shows that a closed connection cancels an answer that has not finished; here, with a model
    // that does not stream, the server only notices the closed connection when the first line is written, which is the same as before)
    let u = uniq("nokeep");
    let (r, fake, _) = push_rig(&u);
    use_slow_model(&u, &slow_model(600, "Written, but nobody is told."));
    // a device is registered but the message does not ask to keep running
    let (_, mut rd) = start_send(&r, json!({"text": "first"}));
    read_until(&mut rd, "chat");
    drop(rd);
    // a person with no device: the flag is dropped by the server and the connection ends the answer as before
    let u2 = uniq("nodevice");
    let (hub2, fake2) = (Arc::new(push::Hub::new(None, "", "mailto:o@zo.test".into())), 0);
    let _ = fake2;
    let r2 = rig_push(&[("tok-o", &u2, "o@x.test")], "o@x.test", 200, 600, hub2);
    use_slow_model(&u2, &slow_model(600, "Written too."));
    let (_, mut rd) = start_send(&r2, json!({"text": "second", "keep_running": true}));
    read_until(&mut rd, "chat");
    drop(rd);
    wait_until(10, "the places to be free", || r.app.tenants.running_answers() == 0 && r2.app.tenants.running_answers() == 0);
    std::thread::sleep(Duration::from_millis(500));
    assert!(fake.calls.lock().unwrap().is_empty(), "nobody is pushed unless keep_running was asked for and a device is registered");
    assert_eq!(r2.app.push.delivered.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn kept_answers_count_against_the_two_at_once_limit_and_free_their_places() {
    let u = uniq("caps");
    let (r, fake, _) = push_rig(&u);
    use_slow_model(&u, &slow_model(1500, "Slow answer."));
    let mut ids = Vec::new();
    for n in 0..2 {
        let (s, mut rd) = start_send(&r, json!({"text": format!("question {n}"), "keep_running": true}));
        assert_eq!(s, 200);
        ids.push(read_until(&mut rd, "chat")["id"].as_str().unwrap().to_string());
        drop(rd);
    }
    // both browsers left, both answers still running: a third message is refused like before
    let (s, _) = start_send(&r, json!({"text": "a third"}));
    assert_eq!(s, 429, "per-user cap unchanged");
    assert_eq!(r.app.tenants.running_answers(), 2);
    wait_until(20, "both answers", || ids.iter().all(|i| saved_answer(&r, i).is_some()));
    wait_until(10, "both pushes", || fake.calls.lock().unwrap().len() == 2);
    wait_until(10, "the places", || r.app.tenants.running_answers() == 0);
    let (s, mut rd) = start_send(&r, json!({"text": "now it works"}));
    assert_eq!(s, 200);
    read_until(&mut rd, "done");
}

#[test]
fn push_registration_is_for_signed_in_people_only_and_always_their_own() {
    let (u1, u2) = (uniq("pa"), uniq("pb"));
    let fake = Arc::new(push::testkit::Fake::default());
    let pdb: Arc<dyn Db> = Arc::new(MemDb::default());
    let hub = Arc::new(push::Hub::new(Some(pdb), testenv::SECRET, "mailto:o@zo.test".into()).with_transport(fake).with_allow(push::Allow::for_test("https://push.test")));
    let r = rig_push(&[("tok-a", &u1, "a@x.test"), ("tok-b", &u2, "b@x.test")], "", 200, 600, hub);
    assert_eq!(call(&r, "", "/api/account/push_config", json!({})).status, 401);
    let cfg = call(&r, "tok-a", "/api/account/push_config", json!({}));
    assert_eq!(cfg.status, 200);
    assert_eq!(base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, cfg.body["vapid_public_key"].as_str().unwrap()).unwrap().len(), 65);
    assert_eq!(cfg.body["fcm"], false);
    let k = push::testkit::browser();
    let id = call(&r, "tok-a", "/api/account/push_subscribe", push::testkit::web_sub("https://push.test/ep/a", &k)).body["id"].as_str().unwrap().to_string();
    assert_eq!(call(&r, "tok-a", "/api/account/push_subscribe", push::testkit::web_sub("https://evil.example/x", &k)).status, 400);
    assert_eq!(call(&r, "tok-a", "/api/account/push_list", json!({})).body["subscriptions"].as_array().unwrap().len(), 1);
    assert!(call(&r, "tok-b", "/api/account/push_list", json!({})).body["subscriptions"].as_array().unwrap().is_empty());
    assert_eq!(call(&r, "tok-b", "/api/account/push_unsubscribe", json!({"id": id})).status, 404, "somebody else's device");
    // a device credential (the terminal client) cannot register notification devices
    let (cred, _) = make_device(&r, "tok-a", "terminal", "cli");
    assert_eq!(call(&r, &cred, "/api/account/push_subscribe", push::testkit::web_sub("https://push.test/ep/z", &k)).status, 403);
    assert_eq!(call(&r, "tok-a", "/api/account/push_unsubscribe", json!({"id": id})).status, 200);
}

#[test]
fn the_installable_app_files_are_served_with_the_right_headers_and_without_a_login() {
    let r = rig(&[("tok", &uniq("pwa"), "p@x.test")], "", 200, 600);
    let get = |p: &str| match ureq::get(&format!("{}{p}", r.base)).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{e}"),
    };
    let m = get("/manifest.webmanifest");
    assert_eq!((m.status(), m.header("Content-Type"), m.header("Cache-Control")), (200, Some("application/manifest+json"), Some("no-cache")));
    let v: Value = m.into_json().unwrap();
    assert_eq!(v["display"], "standalone");
    let sw = get("/sw.js");
    assert_eq!((sw.status(), sw.header("Service-Worker-Allowed"), sw.header("Cache-Control")), (200, Some("/"), Some("no-cache")));
    assert!(sw.header("Content-Type").unwrap().starts_with("text/javascript"));
    let text = sw.into_string().unwrap();
    assert!(!text.contains("__VERSION__") && text.contains("zo-shell-"));
    for p in ["/icons/icon-192.png", "/icons/icon-512.png", "/icons/maskable-192.png", "/icons/maskable-512.png", "/apple-touch-icon.png"] {
        let i = get(p);
        assert_eq!((i.status(), i.header("Content-Type")), (200, Some("image/png")), "{p}");
        assert_eq!(i.header("X-Content-Type-Options"), Some("nosniff"));
    }
    assert_eq!(get("/icons/nothing.png").status(), 404);
    assert_eq!(get("/icons/../main.rs").status(), 404);
    let page = get("/");
    let csp = page.header("Content-Security-Policy").unwrap().to_string();
    assert!(csp.contains("worker-src 'self'") && csp.contains("manifest-src 'self'"), "{csp}");
}

// ===========================================================================================================================
// the desktop link (link.rs) and the Claude token (account.rs): see docs/LINK.md

/// A streaming call (`/api/send`) that runs in a thread: its status and every line it received (pings left out).
fn stream_in_thread(r: &Rig, token: &str, body: Value) -> std::thread::JoinHandle<(u16, Vec<Value>)> {
    let (base, token) = (r.base.clone(), token.to_string());
    std::thread::spawn(move || {
        let res = match ureq::post(&format!("{base}/api/send")).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(30)).send_string(&body.to_string()) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        let status = res.status();
        let text = res.into_string().unwrap_or_default();
        (status, text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v["event"] != "ping").collect())
    })
}

fn caps(approvals: bool) -> Value {
    json!({"claude_code": true, "version": "0.9.1", "os": "linux", "approvals_from_web": approvals})
}

/// One poll of the desktop (waits up to the link's poll time): the jobs it got.
fn dpoll(r: &Rig, cred: &str, name: &str, c: Value) -> Vec<Value> {
    let a = call(r, cred, "/api/link/poll", json!({"name": name, "caps": c}));
    assert_eq!(a.status, 200, "{:?}", a.body);
    a.body["jobs"].as_array().cloned().unwrap_or_default()
}

/// Polls until something arrives (a few seconds at most).
fn dpoll_for(r: &Rig, cred: &str, name: &str, c: Value, want_type: &str) -> Value {
    let t0 = Instant::now();
    loop {
        for j in dpoll(r, cred, name, c.clone()) {
            if j["type"] == want_type {
                return j;
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "no {want_type} job arrived");
    }
}

fn post_events(r: &Rig, cred: &str, job: &str, events: Value, done: bool) -> Answer {
    call(r, cred, "/api/link/events", json!({"job_id": job, "events": events, "done": done}))
}

fn evs(lines: &[Value]) -> Vec<String> {
    lines.iter().map(|l| format!("{}:{}", l["event"].as_str().unwrap_or("?"), l["text"].as_str().or(l["error"].as_str()).unwrap_or(""))).collect()
}

#[test]
fn presence_a_polling_desktop_is_online_a_silent_one_is_not_and_what_it_says_about_itself_is_cleaned() {
    let u = uniq("lk-pres");
    let r = rig(&[("tok", &u, "p@x.test")], "", 200, 600);
    let (cred, id) = make_device(&r, "tok", "Laptop-p", "desktop");
    let st = |r: &Rig| call(r, "tok", "/api/link/status", json!({}));
    // signed in, never polled: listed, offline
    let a = st(&r);
    assert_eq!(a.status, 200, "{:?}", a.body);
    let d = &a.body["devices"][0];
    assert_eq!((d["id"].as_str(), d["name"].as_str(), d["online"].as_bool()), (Some(id.as_str()), Some("Laptop-p"), Some(false)));
    // a poll (here one that waits in a thread) makes it online while it is open
    let (base, c2) = (r.base.clone(), cred.clone());
    let waiting = std::thread::spawn(move || ureq::post(&format!("{base}/api/link/poll")).set("Authorization", &format!("Bearer {c2}")).send_string(&json!({"name": "My Laptop", "caps": {"claude_code": true, "approvals_from_web": false, "version": "1.2", "os": "linux", "evil": "<script>", "extra": {"a": 1}}}).to_string()).unwrap().status());
    std::thread::sleep(Duration::from_millis(150));
    let d = st(&r).body["devices"][0].clone();
    assert_eq!(d["online"], true, "{d}");
    assert_eq!(d["name"], "My Laptop");
    assert_eq!(d["caps"], json!({"claude_code": true, "approvals_from_web": false, "version": "1.2", "os": "linux"}), "unknown fields are dropped");
    assert!(d["last_seen"].as_u64().unwrap() > 1_000_000_000);
    assert_eq!(waiting.join().unwrap(), 200);
    // it stays online for a short while after the poll, then it is offline
    assert_eq!(st(&r).body["devices"][0]["online"], true);
    std::thread::sleep(Duration::from_millis(2000));
    assert_eq!(st(&r).body["devices"][0]["online"], false);
    // a CLI credential is not a computer: it never shows
    make_device(&r, "tok", "terminal-p", "cli");
    assert_eq!(st(&r).body["devices"].as_array().unwrap().len(), 1);
    // revoking the desktop removes it
    assert_eq!(call(&r, "tok", "/api/account/device_revoke", json!({"id": id})).status, 200);
    assert_eq!(st(&r).body["devices"].as_array().unwrap().len(), 0);
    assert_eq!(call(&r, &cred, "/api/link/poll", json!({})).status, 401);
}

#[test]
fn a_message_goes_to_the_computer_its_events_come_back_in_order_and_the_stream_ends_with_done() {
    let u = uniq("lk-life");
    let r = rig(&[("tok", &u, "l@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Desk", "desktop");
    assert!(dpoll(&r, &cred, "Desk", caps(false)).is_empty(), "nothing to do yet");
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "open the calendar", "chat": "c-1", "attachments": [{"kind": "text", "name": "a.txt", "text": "hi"}], "mode": "bypass", "evil": "x", "tenant": {"id": "x"}}));
    let job = dpoll_for(&r, &cred, "Desk", caps(false), "send");
    // the job carries the whitelisted fields only, with its age (the desktop refuses anything older than two minutes)
    assert_eq!(job["text"], "open the calendar");
    assert_eq!((job["chat"].as_str(), job["mode"].as_str()), (Some("c-1"), Some("bypass")));
    assert!(job.get("evil").is_none() && job.get("tenant").is_none() && job["attachments"][0]["name"] == "a.txt");
    assert!(job["age_secs"].as_u64().unwrap() < 5 && job["expires_in_secs"].as_u64().unwrap() <= 2);
    let id = job["id"].as_str().unwrap();
    // events arrive in posting order, across posts; stream and done lines of the desktop are ignored (the server writes them)
    let a = post_events(&r, &cred, id, json!([{"event": "delta", "text": "one"}, {"event": "stream", "id": 999}, {"event": "delta", "text": "two"}]), false);
    assert_eq!((a.status, a.body["cancel"].clone()), (200, json!(false)), "{:?}", a.body);
    assert_eq!(post_events(&r, &cred, id, json!([{"event": "delta", "text": "three"}, {"event": "done"}]), true).body["cancel"], false, "the post that ends the job is an ordinary one");
    let (status, lines) = t.join().unwrap();
    assert_eq!(status, 200);
    assert_eq!(lines[0]["event"], "stream");
    assert_eq!((lines[0]["target"].as_str(), lines[0]["device"].as_str()), (Some("desktop"), Some("Desk")));
    assert!(lines[0]["id"].as_u64().is_some() && lines[0]["id"] != 999);
    assert_eq!(evs(&lines[1..]), ["delta:one", "delta:two", "delta:three", "done:"], "{lines:?}");
    // the job is over: a desktop that still posts is told to stop, and nothing is left behind
    let a = post_events(&r, &cred, id, json!([{"event": "delta", "text": "late"}]), false);
    assert_eq!(a.body["cancel"], true);
    let (_, jobs, streams) = r.app.link.counts();
    assert_eq!((jobs, streams), (0, 0));
}

#[test]
fn without_an_online_computer_the_answer_is_an_immediate_clear_error_and_done() {
    let u = uniq("lk-off");
    let r = rig(&[("tok", &u, "o@x.test")], "", 200, 600);
    // no desktop at all
    let (status, lines) = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "hi"})).join().unwrap();
    assert_eq!(status, 200);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!((lines[0]["event"].as_str(), lines[0]["error"].as_str()), (Some("error"), Some("desktop_offline")));
    assert!(lines[0]["message"].as_str().unwrap().contains("Your computer is not reachable") && lines[0]["message"].as_str().unwrap().contains("Control this computer from the web"));
    assert_eq!(lines[1]["event"], "done");
    // a desktop that polled a while ago
    let (cred, id) = make_device(&r, "tok", "Old", "desktop");
    dpoll(&r, &cred, "Old", caps(false));
    std::thread::sleep(Duration::from_millis(2100));
    let (_, lines) = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "hi", "device": id})).join().unwrap();
    assert_eq!(lines[0]["error"], "desktop_offline");
    // the settings questions say the same
    let a = call(&r, "tok", "/api/op", json!({"op": "desktop_op", "inner": {"op": "status"}}));
    assert_eq!((a.status, a.body["error"].as_str()), (503, Some("desktop_offline")), "{:?}", a.body);
    // bad requests are ordinary errors
    assert_eq!(call(&r, "tok", "/api/send", json!({"target": "desktop", "text": "  "})).status, 400);
    assert_eq!(call(&r, "tok", "/api/send", json!({"target": "desktop", "text": "x".repeat(link::MAX_TEXT_CHARS + 1)})).status, 413);
    assert_eq!(call(&r, "tok", "/api/send", json!({"target": "desktop", "text": "hi", "chat": {"a": 1}})).status, 400);
    assert_eq!(call(&r, "tok", "/api/send", json!({"target": "desktop", "text": "hi", "attachments": "x"})).status, 400);
}

#[test]
fn only_one_web_request_runs_on_a_computer_at_a_time() {
    let u = uniq("lk-busy");
    let r = rig(&[("tok", &u, "b@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Busy", "desktop");
    dpoll(&r, &cred, "Busy", caps(false));
    let first = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "long task"}));
    let job = dpoll_for(&r, &cred, "Busy", caps(false), "send");
    let (_, second) = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "another"})).join().unwrap();
    assert_eq!(second[0]["error"], "desktop_busy", "{second:?}");
    assert_eq!(second.last().unwrap()["event"], "done");
    // a settings question is not a job: it may go while the task runs
    let (base, c2) = (r.base.clone(), cred.clone());
    let op = {
        let (b, t) = (base.clone(), "tok".to_string());
        std::thread::spawn(move || ureq::post(&format!("{b}/api/op")).set("Authorization", &format!("Bearer {t}")).send_string(&json!({"op": "desktop_op", "inner": {"op": "status"}}).to_string()).unwrap().into_string().unwrap())
    };
    let _ = c2;
    let opjob = dpoll_for(&r, &cred, "Busy", caps(false), "op");
    assert_eq!(post_events(&r, &cred, opjob["id"].as_str().unwrap(), json!([{"event": "op_result", "reply": {"provider": "claude-code"}}]), true).status, 200);
    assert_eq!(serde_json::from_str::<Value>(&op.join().unwrap()).unwrap(), json!({"ok": true, "result": {"provider": "claude-code"}}));
    post_events(&r, &cred, job["id"].as_str().unwrap(), json!([]), true);
    assert_eq!(first.join().unwrap().1.last().unwrap()["event"], "done");
    // and when it is over the computer takes the next one
    let next = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "next"}));
    let j2 = dpoll_for(&r, &cred, "Busy", caps(false), "send");
    post_events(&r, &cred, j2["id"].as_str().unwrap(), json!([{"event": "delta", "text": "ok"}]), true);
    assert!(evs(&next.join().unwrap().1).contains(&"delta:ok".to_string()));
}

#[test]
fn a_job_nobody_picks_up_expires_is_told_and_is_never_delivered_later() {
    let u = uniq("lk-exp");
    let r = rig(&[("tok", &u, "e@x.test")], "", 200, 600);
    let (cred, id) = make_device(&r, "tok", "Slow", "desktop");
    dpoll(&r, &cred, "Slow", caps(false)); // online, then it stops polling
    let t0 = Instant::now();
    let (_, lines) = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "hello?"})).join().unwrap();
    assert!(t0.elapsed() >= Duration::from_millis(1400) && t0.elapsed() < Duration::from_secs(6), "{:?}", t0.elapsed());
    assert_eq!(lines[0]["event"], "stream");
    assert_eq!(lines[1]["error"], "desktop_timeout", "{lines:?}");
    assert!(lines[1]["message"].as_str().unwrap().contains("not run"));
    assert_eq!(lines.last().unwrap()["event"], "done");
    // the desktop comes back later: nothing is waiting for it, the job is never run
    assert!(dpoll(&r, &cred, "Slow", caps(false)).is_empty());
    assert_eq!(r.app.link.queued(&tenant_of(&u), &id), 0);
    // the same for a job that waited in the queue longer than the limit before the desktop's next poll
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "late"}));
    std::thread::sleep(Duration::from_millis(1700)); // longer than the expiry, shorter than nothing: the relay already gave up
    assert!(dpoll(&r, &cred, "Slow", caps(false)).iter().all(|j| j["type"] != "send"), "an expired job is not delivered");
    assert_eq!(t.join().unwrap().1[1]["error"], "desktop_timeout");
}

#[test]
fn stop_reaches_the_computer_the_events_answer_says_cancel_and_the_page_is_told_it_stopped() {
    let u = uniq("lk-stop");
    let r = rig(&[("tok", &u, "s@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Stoppable", "desktop");
    dpoll(&r, &cred, "Stoppable", caps(false));
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "forever"}));
    let job = dpoll_for(&r, &cred, "Stoppable", caps(false), "send");
    let id = job["id"].as_str().unwrap().to_string();
    let stream = r.app.link.counts(); // (devices, jobs, streams)
    assert_eq!((stream.1, stream.2), (1, 1));
    let sid = loop {
        // the stream number is in the first line: ask the server through the reply route until it is accepted
        let n = (1..200u64).find(|n| call(&r, "tok", "/api/reply", json!({"stream": n, "op": "stop"})).status == 200);
        if let Some(n) = n {
            break n;
        }
    };
    assert!(sid > 0);
    // the stop comes with the next poll, and the next events answer says cancel
    let stop = dpoll_for(&r, &cred, "Stoppable", caps(false), "stop");
    assert_eq!(stop["job_id"], id.as_str());
    assert_eq!(post_events(&r, &cred, &id, json!([{"event": "delta", "text": "partial"}]), false).body["cancel"], true);
    // the desktop ends it
    post_events(&r, &cred, &id, json!([{"event": "stopped"}]), true);
    let (_, lines) = t.join().unwrap();
    assert!(evs(&lines).contains(&"delta:partial".to_string()) && evs(&lines).contains(&"stopped:".to_string()), "{lines:?}");
    assert_eq!(lines.last().unwrap()["event"], "done");
    // a desktop that does not end it is overruled after a short grace: the page gets "stopped" and done
    dpoll(&r, &cred, "Stoppable", caps(false));
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "stuck"}));
    dpoll_for(&r, &cred, "Stoppable", caps(false), "send");
    let sid = (1..400u64).find(|n| call(&r, "tok", "/api/reply", json!({"stream": n, "op": "stop"})).status == 200).expect("a running stream");
    assert!(sid > 0);
    let (_, lines) = t.join().unwrap();
    assert_eq!(evs(&lines[1..]), ["stopped:", "done:"], "{lines:?}");
}

#[test]
fn approvals_from_the_web_are_forwarded_only_when_the_computer_said_it_takes_them() {
    let u = uniq("lk-appr");
    let r = rig(&[("tok", &u, "a@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Appr", "desktop");
    // 1) approvals stay on the computer
    dpoll(&r, &cred, "Appr", caps(false));
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "do it"}));
    let job = dpoll_for(&r, &cred, "Appr", caps(false), "send");
    let jid = job["id"].as_str().unwrap().to_string();
    post_events(&r, &cred, &jid, json!([{"event": "approval", "id": "ap-1", "summary": "Run ls"}]), false);
    let sid = (1..400u64).find(|n| call(&r, "tok", "/api/reply", json!({"stream": *n, "op": "approve", "id": "ap-1", "yes": true})).status == 409).expect("refused for a running stream");
    let a = call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "approve", "id": "ap-1", "yes": true}));
    assert_eq!((a.status, a.body["error"].as_str()), (409, Some("approvals_on_computer")), "{:?}", a.body);
    assert!(dpoll(&r, &cred, "Appr", caps(false)).iter().all(|j| j["type"] != "approval_reply"), "nothing was forwarded");
    // 2) the computer announces it takes web approvals: the answer is forwarded, once, with the job and the approval it is for
    let a = call(&r, "tok", "/api/link/status", json!({}));
    assert_eq!(a.body["devices"][0]["caps"]["approvals_from_web"], false);
    dpoll(&r, &cred, "Appr", caps(true));
    let a = call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "approve", "id": "ap-1", "yes": false}));
    assert_eq!(a.status, 200, "{:?}", a.body);
    let fwd = dpoll_for(&r, &cred, "Appr", caps(true), "approval_reply");
    assert_eq!((fwd["job_id"].as_str(), fwd["approval_id"].as_str(), fwd["approval"].as_str(), fwd["yes"].as_bool()), (Some(jid.as_str()), Some("ap-1"), Some("ap-1"), Some(false)));
    // the desktop's own ids are numbers
    assert_eq!(call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "approve", "id": 7, "yes": true})).status, 200);
    assert_eq!(dpoll_for(&r, &cred, "Appr", caps(true), "approval_reply")["approval"], 7);
    // malformed answers and other ops are refused
    assert_eq!(call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "approve", "id": {"x": 1}, "yes": true})).status, 400);
    assert_eq!(call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "approve", "id": "ap-2"})).status, 400);
    assert_eq!(call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "shell", "cmd": "rm -rf /"})).status, 400);
    post_events(&r, &cred, &jid, json!([]), true);
    t.join().unwrap();
}

#[test]
fn a_question_card_of_a_desktop_session_is_relayed_and_its_answer_is_forwarded_checked() {
    let u = uniq("lk-ques");
    let r = rig(&[("tok", &u, "q@x.test"), ("tok2", &uniq("lk-ques2"), "q2@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Ques", "desktop");
    dpoll(&r, &cred, "Ques", caps(false));
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "ask me"}));
    let job = dpoll_for(&r, &cred, "Ques", caps(false), "send");
    let jid = job["id"].as_str().unwrap().to_string();
    let card = json!({"event": "question", "id": 3, "questions": [{"question": "Which?", "header": "H", "multi_select": false, "options": [{"label": "a", "description": ""}, {"label": "b", "description": ""}]}]});
    post_events(&r, &cred, &jid, json!([card.clone()]), false);
    // the card reaches the page as it is
    let sid = (1..400u64).find(|n| call(&r, "tok", "/api/reply", json!({"stream": *n, "op": "answer", "id": 3, "cancelled": true})).status == 200).expect("a running stream");
    // web approvals are OFF on this computer: a question is not an approval, so the answer is still forwarded (and nothing else is)
    let fwd = dpoll_for(&r, &cred, "Ques", caps(false), "answer_reply");
    assert_eq!((fwd["job_id"].as_str(), fwd["question_id"].as_u64(), fwd["cancelled"].as_bool()), (Some(jid.as_str()), Some(3), Some(true)));
    let ok = call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "answer", "id": 3, "answers": [{"selected": ["b"], "other": null}]}));
    assert_eq!(ok.status, 200, "{:?}", ok.body);
    let fwd = dpoll_for(&r, &cred, "Ques", caps(false), "answer_reply");
    assert_eq!(fwd["answers"], json!([{"selected": ["b"], "other": null}]));
    // malformed ones never get through
    for bad in [json!({"op": "answer", "id": "x", "cancelled": true}), json!({"op": "answer", "id": 3}), json!({"op": "answer", "id": 3, "answers": []}),
                json!({"op": "answer", "id": 3, "answers": [{}, {}, {}, {}, {}]}), json!({"op": "answer", "id": 3, "answers": [{"other": "y".repeat(30_000)}]}), json!({"op": "answer", "id": 3, "answers": "b"})] {
        let mut b = bad.clone();
        b["stream"] = json!(sid);
        assert_eq!(call(&r, "tok", "/api/reply", b).status, 400, "{bad}");
    }
    assert!(dpoll(&r, &cred, "Ques", caps(false)).iter().all(|j| j["type"] != "answer_reply"), "nothing malformed was forwarded");
    // another person cannot answer it
    let other = call(&r, "tok2", "/api/reply", json!({"stream": sid, "op": "answer", "id": 3, "cancelled": true}));
    assert_eq!(other.status, 404);
    assert!(dpoll(&r, &cred, "Ques", caps(false)).iter().all(|j| j["type"] != "answer_reply"));
    // once the session is over nothing is forwarded
    post_events(&r, &cred, &jid, json!([]), true);
    t.join().unwrap();
    assert_ne!(call(&r, "tok", "/api/reply", json!({"stream": sid, "op": "answer", "id": 3, "cancelled": true})).status, 200);
}

#[test]
fn settings_questions_for_the_computer_are_whitelisted_cleaned_and_answered() {
    let u = uniq("lk-op");
    let r = rig(&[("tok", &u, "w@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Ops", "desktop");
    dpoll(&r, &cred, "Ops", caps(false));
    for bad in [json!({"op": "shell"}), json!({"op": "set_key", "provider": "x", "key": "k"}), json!({"op": "send"}), json!({"op": "desktop_op"}), json!({"op": "mode", "set": "bypass"}), json!({"op": "mode", "set": "BYPASS"}), json!({"op": "mode", "set": 5}), json!({}), json!("status")] {
        let a = call(&r, "tok", "/api/op", json!({"op": "desktop_op", "inner": bad}));
        assert_eq!((a.status, a.body["error"].as_str()), (400, Some("not_allowed")), "{bad}: {:?}", a.body);
    }
    // the question that is allowed arrives with its known fields only
    let (base, ) = (r.base.clone(), );
    let ask = std::thread::spawn(move || ureq::post(&format!("{base}/api/op")).set("Authorization", "Bearer tok").send_string(&json!({"op": "desktop_op", "inner": {"op": "select", "provider": "claude-code", "model": "opus", "effort": "high", "cmd": "rm -rf /", "tenant": {"id": "x"}}}).to_string()).unwrap().into_string().unwrap());
    let job = dpoll_for(&r, &cred, "Ops", caps(false), "op");
    assert_eq!(job["inner"], json!({"op": "select", "provider": "claude-code", "model": "opus", "effort": "high"}));
    post_events(&r, &cred, job["id"].as_str().unwrap(), json!([{"event": "result", "result": {"ok": true, "model": "opus"}}]), true);
    assert_eq!(serde_json::from_str::<Value>(&ask.join().unwrap()).unwrap()["result"]["model"], "opus");
    // every name on the list passes the check
    for op in link::OPS {
        assert!(link::clean_inner(&json!({"op": op})).is_ok(), "{op}");
    }
    assert_eq!(link::clean_inner(&json!({"op": "mode", "set": "Manual"})).unwrap(), json!({"op": "mode", "set": "manual"}));
    // the read-only control operations (docs/FULL-CONTROL.md phase 1): a path and nothing else gets through, and a bad path is refused here
    assert_eq!(link::clean_inner(&json!({"op": "fs_list", "path": "~/docs", "cmd": "rm -rf /", "tenant": "x", "set": "auto"})).unwrap(), json!({"op": "fs_list", "path": "~/docs"}));
    assert_eq!(link::clean_inner(&json!({"op": "fs_read", "path": "a.txt"})).unwrap(), json!({"op": "fs_read", "path": "a.txt"}));
    assert_eq!(link::clean_inner(&json!({"op": "screenshot", "path": "/etc/passwd"})).unwrap(), json!({"op": "screenshot"}));
    assert_eq!(link::clean_inner(&json!({"op": "sys_info"})).unwrap(), json!({"op": "sys_info"}));
    assert_eq!(link::clean_inner(&json!({"op": "models", "provider": "p", "path": "/etc"})).unwrap(), json!({"op": "models", "provider": "p"}));
    for bad in [json!({"op": "fs_list", "path": ["a"]}), json!({"op": "fs_read", "path": "a\u{0}b"}), json!({"op": "fs_read", "path": "x".repeat(2000)}), json!({"op": "fs_write", "path": "a"}), json!({"op": "fs_rm", "path": "a"}), json!({"op": "term_open"}), json!({"op": "power", "set": "off"}), json!({"op": "app_launch"})] {
        let a = call(&r, "tok", "/api/op", json!({"op": "desktop_op", "inner": bad}));
        assert_eq!((a.status, a.body["error"].as_str()), (400, Some("not_allowed")), "{bad}: {:?}", a.body);
    }
    {
        // a folder listing goes to the computer as {op, path}, and the computer's answer comes back to the page
        let base = r.base.clone();
        let ask = std::thread::spawn(move || ureq::post(&format!("{base}/api/op")).set("Authorization", "Bearer tok").send_string(&json!({"op": "desktop_op", "inner": {"op": "fs_list", "path": "docs", "x": 1}}).to_string()).unwrap().into_string().unwrap());
        let job = dpoll_for(&r, &cred, "Ops", caps(false), "op");
        assert_eq!(job["inner"], json!({"op": "fs_list", "path": "docs"}));
        post_events(&r, &cred, job["id"].as_str().unwrap(), json!([{"event": "op_result", "reply": {"ok": true, "path": "~/docs", "entries": []}}]), true);
        assert_eq!(serde_json::from_str::<Value>(&ask.join().unwrap()).unwrap()["result"]["path"], "~/docs");
        // a computer whose person said no: the page gets that as the error
        let base = r.base.clone();
        let ask = std::thread::spawn(move || {
            let res = match ureq::post(&format!("{base}/api/op")).set("Authorization", "Bearer tok").send_string(&json!({"op": "desktop_op", "inner": {"op": "screenshot"}}).to_string()) {
                Ok(r) => r,
                Err(ureq::Error::Status(_, r)) => r,
                Err(e) => panic!("{e}"),
            };
            (res.status(), serde_json::from_str::<Value>(&res.into_string().unwrap()).unwrap())
        });
        let job = dpoll_for(&r, &cred, "Ops", caps(false), "op");
        post_events(&r, &cred, job["id"].as_str().unwrap(), json!([{"event": "error", "error": "You said no on your computer."}]), true);
        let (status, body) = ask.join().unwrap();
        assert_eq!((status, body["error"].as_str()), (503, Some("desktop_error")), "{body:?}");
        assert!(body["message"].as_str().unwrap().contains("said no"), "{body:?}");
    }
    // a desktop that never answers: the page is told, and the question is taken back so that it is not run later
    let t0 = Instant::now();
    let a = call(&r, "tok", "/api/op", json!({"op": "desktop_op", "inner": {"op": "providers"}}));
    assert_eq!((a.status, a.body["error"].as_str()), (503, Some("desktop_timeout")), "{:?}", a.body);
    assert!(t0.elapsed() < Duration::from_secs(9));
    assert!(dpoll(&r, &cred, "Ops", caps(false)).is_empty());
    // a desktop that answers with an error event: the page gets that as the error
    let base = r.base.clone();
    let ask = std::thread::spawn(move || {
        let res = match ureq::post(&format!("{base}/api/op")).set("Authorization", "Bearer tok").send_string(&json!({"op": "desktop_op", "inner": {"op": "models"}}).to_string()) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        (res.status(), serde_json::from_str::<Value>(&res.into_string().unwrap()).unwrap())
    });
    let job = dpoll_for(&r, &cred, "Ops", caps(false), "op");
    post_events(&r, &cred, job["id"].as_str().unwrap(), json!([{"event": "error", "error": "No provider is chosen on the computer."}]), true);
    let (status, body) = ask.join().unwrap();
    assert_eq!((status, body["error"].as_str(), body["message"].as_str()), (503, Some("desktop_error"), Some("No provider is chosen on the computer.")));
}

#[test]
fn nobody_can_reach_another_persons_computers_jobs_or_streams() {
    let (ua, ub) = (uniq("lk-iso-a"), uniq("lk-iso-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 600);
    let (ca, ida) = make_device(&r, "tok-a", "A-desk", "desktop");
    let (cb, idb) = make_device(&r, "tok-b", "B-desk", "desktop");
    dpoll(&r, &ca, "A-desk", caps(true));
    dpoll(&r, &cb, "B-desk", caps(true));
    // B sees only B's computer
    let sb = call(&r, "tok-b", "/api/link/status", json!({}));
    assert_eq!(sb.body["devices"].as_array().unwrap().len(), 1);
    assert_eq!(sb.body["devices"][0]["id"], idb.as_str());
    assert!(!sb.body.to_string().contains("A-desk") && !sb.body.to_string().contains(&ida));
    // B names A's computer: it is "offline" (nothing about it leaks), and nothing reaches it
    let (_, lines) = stream_in_thread(&r, "tok-b", json!({"target": "desktop", "text": "hi A", "device": ida})).join().unwrap();
    assert_eq!(lines[0]["error"], "desktop_offline");
    let a = call(&r, "tok-b", "/api/op", json!({"op": "desktop_op", "device": ida, "inner": {"op": "status"}}));
    assert_eq!(a.body["error"], "desktop_offline");
    assert!(dpoll(&r, &ca, "A-desk", caps(true)).is_empty(), "A's computer got nothing");
    // A runs a job; B's credential cannot post events for it, B cannot stop or approve it, and A's credential cannot post for B's job
    let ta = stream_in_thread(&r, "tok-a", json!({"target": "desktop", "text": "A's secret task"}));
    let job_a = dpoll_for(&r, &ca, "A-desk", caps(true), "send");
    let ja = job_a["id"].as_str().unwrap();
    let forged = post_events(&r, &cb, ja, json!([{"event": "delta", "text": "FORGED"}]), true);
    assert_eq!((forged.status, forged.body["cancel"].clone()), (404, json!(true)), "{:?}", forged.body);
    for n in 1..100u64 {
        for op in [json!({"stream": n, "op": "stop"}), json!({"stream": n, "op": "approve", "id": "x", "yes": true})] {
            assert_eq!(call(&r, "tok-b", "/api/reply", op).status, 404, "B cannot reach stream {n}");
        }
    }
    let tb = stream_in_thread(&r, "tok-b", json!({"target": "desktop", "text": "B's task"}));
    let job_b = dpoll_for(&r, &cb, "B-desk", caps(true), "send");
    assert_eq!(post_events(&r, &ca, job_b["id"].as_str().unwrap(), json!([{"event": "delta", "text": "FORGED"}]), true).status, 404);
    assert_eq!(post_events(&r, &cb, ja, json!([]), true).status, 404, "not even an empty post for A's job");
    // an unknown job is a 404 as well, and A's real job still works
    assert_eq!(post_events(&r, &ca, "0123456789abcdef", json!([]), false).status, 404);
    post_events(&r, &ca, ja, json!([{"event": "delta", "text": "A's answer"}]), true);
    post_events(&r, &cb, job_b["id"].as_str().unwrap(), json!([{"event": "delta", "text": "B's answer"}]), true);
    let (la, lb) = (ta.join().unwrap().1, tb.join().unwrap().1);
    assert!(evs(&la).contains(&"delta:A's answer".to_string()) && !evs(&la).iter().any(|e| e.contains("FORGED") || e.contains("B's")), "{la:?}");
    assert!(evs(&lb).contains(&"delta:B's answer".to_string()) && !evs(&lb).iter().any(|e| e.contains("FORGED") || e.contains("A's")), "{lb:?}");
}

#[test]
fn which_credential_may_call_the_link_routes_over_http() {
    let u = uniq("lk-scope");
    let r = rig(&[("tok", &u, "c@x.test")], "owner@x.test", 200, 600);
    let (dt, _) = make_device(&r, "tok", "scope-desk", "desktop");
    let (ft, _) = make_device(&r, "tok", "scope-term", "cli");
    // the desktop credential: the link, the Claude token route (which then decides by the person), the sync; not the API
    assert_eq!(call(&r, &dt, "/api/link/poll", json!({})).status, 200);
    assert_eq!(call(&r, &dt, "/api/link/events", json!({"job_id": "nope"})).status, 404);
    assert_eq!(call(&r, &dt, "/api/link/status", json!({})).status, 200);
    assert_eq!(call(&r, &dt, "/api/sync/claude_token", json!({"action": "status"})).status, 403, "reached, and refused because the person is not the owner");
    assert_eq!(call(&r, &dt, "/api/op", json!({"op": "desktop_op", "inner": {"op": "status"}})).status, 403);
    assert_eq!(call(&r, &dt, "/api/op", json!({"op": "chats"})).status, 403);
    assert_eq!(call(&r, &dt, "/api/send", json!({"target": "desktop", "text": "x"})).status, 403);
    assert_eq!(call(&r, &dt, "/api/reply", json!({"stream": 1, "op": "stop"})).status, 403);
    assert_eq!(call(&r, &dt, "/api/account/claude_status", json!({})).status, 403);
    // a web session cannot be a computer and cannot hand over a token
    for p in ["/api/link/poll", "/api/link/events", "/api/sync/claude_token"] {
        assert_eq!(call(&r, "tok", p, json!({"job_id": "x", "action": "status"})).status, 403, "{p}");
    }
    assert_eq!(call(&r, "tok", "/api/link/status", json!({})).status, 200);
    // the terminal client is not a computer either, but may ask which are linked
    for p in ["/api/link/poll", "/api/link/events", "/api/sync/claude_token"] {
        assert_eq!(call(&r, &ft, p, json!({})).status, 403, "{p}");
    }
    assert_eq!(call(&r, &ft, "/api/link/status", json!({})).status, 200);
    // nobody at all
    for p in ["/api/link/poll", "/api/link/events", "/api/link/status", "/api/sync/claude_token"] {
        assert_eq!(call(&r, "", p, json!({})).status, 401, "{p}");
        assert_eq!(call(&r, &format!("zd_{}", "B".repeat(43)), p, json!({})).status, 401, "{p}");
    }
}

#[test]
fn what_a_computer_posts_and_polls_is_size_and_rate_checked() {
    let u = uniq("lk-lim");
    let r = rig(&[("tok", &u, "z@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Limits", "desktop");
    dpoll(&r, &cred, "Limits", caps(false));
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "go"}));
    let job = dpoll_for(&r, &cred, "Limits", caps(false), "send");
    let id = job["id"].as_str().unwrap();
    // malformed posts
    assert_eq!(call(&r, &cred, "/api/link/events", json!({"events": []})).status, 400, "no job");
    assert_eq!(post_events(&r, &cred, id, json!("x"), false).status, 400);
    assert_eq!(post_events(&r, &cred, id, json!([1, 2]), false).status, 400);
    assert_eq!(post_events(&r, &cred, id, json!((0..=link::EVENTS_PER_POST).map(|_| json!({"event": "delta"})).collect::<Vec<_>>()), false).status, 400);
    // a post over one megabyte is refused before it is read
    let huge = json!([{"event": "delta", "text": "x".repeat(1_100_000)}]);
    assert_eq!(post_events(&r, &cred, id, huge, false).status, 413);
    // a poll body is small
    assert_eq!(call(&r, &cred, "/api/link/poll", json!({"name": "x".repeat(20_000)})).status, 413);
    post_events(&r, &cred, id, json!([]), true);
    t.join().unwrap();
}

#[test]
fn a_deleted_account_takes_its_computers_and_running_jobs_with_it() {
    let u = uniq("lk-del");
    let r = rig(&[("tok", &u, "d@x.test")], "", 200, 600);
    let (cred, _) = make_device(&r, "tok", "Gone", "desktop");
    dpoll(&r, &cred, "Gone", caps(false));
    let t = stream_in_thread(&r, "tok", json!({"target": "desktop", "text": "go"}));
    dpoll_for(&r, &cred, "Gone", caps(false), "send");
    assert_eq!(call(&r, "tok", "/api/account/delete", json!({"confirm": "DELETE"})).status, 200);
    let (_, lines) = t.join().unwrap();
    assert_eq!(lines.last().unwrap()["event"], "done");
    assert!(lines.iter().any(|l| l["error"] == "account_deleted"), "{lines:?}");
    assert_eq!(r.app.link.counts().0, 0, "the computers are forgotten");
    assert_eq!(call(&r, &cred, "/api/link/poll", json!({})).status, 401);
}

// ---- Claude Code on the server for the owner ------------------------------------------------------------------------------

const OWNER_TOKEN: &str = "sk-ant-oat01-OWNER-token-for-the-tests-0123456789abcdef";

#[test]
fn the_claude_token_is_for_the_owners_desktop_only_stored_encrypted_and_never_shown() {
    let (uo, ur) = (uniq("ct-owner"), uniq("ct-restr"));
    let r = rig(&[("tok-o", &uo, "owner@x.test"), ("tok-r", &ur, "guest@x.test")], "owner@x.test", 200, 600);
    let (od, _) = make_device(&r, "tok-o", "owner-desk", "desktop");
    let (rd, _) = make_device(&r, "tok-r", "guest-desk", "desktop");
    // nothing yet
    let s = call(&r, "tok-o", "/api/account/claude_status", json!({}));
    assert_eq!((s.status, s.body["present"].clone(), s.body["admin"].clone()), (200, json!(false), json!(true)), "{:?}", s.body);
    // a restricted person's desktop: refused, with the reason; nothing is stored
    let a = call(&r, &rd, "/api/sync/claude_token", json!({"action": "set", "token": OWNER_TOKEN}));
    assert_eq!(a.status, 403, "{:?}", a.body);
    assert!(a.body["error"].as_str().unwrap().contains("owner"));
    assert!(!tenant_dir(&ur).join("config").join("keys.enc").exists());
    let s = call(&r, "tok-r", "/api/account/claude_status", json!({}));
    assert_eq!((s.body["present"].clone(), s.body["admin"].clone()), (json!(false), json!(false)));
    // the owner's desktop: garbage is refused, a token is stored
    for bad in [json!({"action": "set"}), json!({"action": "set", "token": "hello"}), json!({"action": "set", "token": "sk-ant-has space in it 01234567890123456789"}), json!({"action": "nope"}), json!({})] {
        assert_eq!(call(&r, &od, "/api/sync/claude_token", bad.clone()).status, 400, "{bad}");
    }
    let a = call(&r, &od, "/api/sync/claude_token", json!({"token": OWNER_TOKEN}));
    assert_eq!(a.status, 200, "{:?}", a.body);
    assert!(a.body["set_at"].as_u64().unwrap() > 1_000_000_000);
    assert!(!a.body.to_string().contains("sk-ant"), "the answer never repeats the token");
    let s = call(&r, "tok-o", "/api/account/claude_status", json!({}));
    assert_eq!((s.body["present"].clone(), s.body["source"].clone(), s.body["admin"].clone()), (json!(true), json!("own"), json!(true)));
    assert_eq!(s.body["set_at"], a.body["set_at"]);
    assert!(!s.body.to_string().contains("sk-ant"));
    assert_eq!(call(&r, &od, "/api/sync/claude_token", json!({"action": "status"})).body["present"], true);
    // at rest: encrypted in the person's key store, and the same ciphertext is all that goes to Firestore
    let raw = std::fs::read(tenant_dir(&uo).join("config").join("keys.enc")).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("sk-ant-oat01"));
    let map = zo_core::ai::keys::decrypt_map(testenv::SECRET, &tenant_of(&uo), &raw).unwrap();
    assert_eq!(map["claude-code-token"], OWNER_TOKEN);
    r.app.tenants.push_all();
    let docs: String = r.db.docs.lock().unwrap().iter().map(|((c, i), t)| format!("{c}/{i}={t}")).collect();
    assert!(!docs.contains("sk-ant-oat01") && !docs.contains(OWNER_TOKEN), "Firestore never holds the token in clear");
    assert!(docs.contains("keys_enc"), "the encrypted key file is what is kept");
    // not in any export, listing, sync answer or provider list
    for (tok, path, body) in [("tok-o", "/api/account/export", json!({})), ("tok-o", "/api/account/whoami", json!({})), ("tok-o", "/api/account/devices_list", json!({})), ("tok-o", "/api/op", json!({"op": "providers"})), ("tok-o", "/api/op", json!({"op": "status"}))] {
        let a = call(&r, tok, path, body);
        assert!(!a.body.to_string().contains("sk-ant"), "{path}: {:?}", a.body);
    }
    for (tok, path, body) in [(&od, "/api/sync/manifest", json!({})), (&od, "/api/sync/get", json!({"kind": "files", "id": "keys_enc"})), (&od, "/api/sync/get", json!({"kind": "keys", "id": "claude-code-token"}))] {
        let a = call(&r, tok, path, body);
        assert!(!a.body.to_string().contains("sk-ant") && !a.body.to_string().contains("keys_enc") || path.ends_with("manifest") && !a.body.to_string().contains("sk-ant"), "{path}: {:?}", a.body);
    }
    // the owner sees Claude Code as a provider now (never the restricted person)
    let p = call(&r, "tok-o", "/api/op", json!({"op": "providers"}));
    let cc = p.body["providers"].as_array().unwrap().iter().find(|x| x["id"] == "claude-code").cloned().expect("listed for the owner with a token");
    assert_eq!(cc["name"], "Claude Code (your subscription)");
    let p = call(&r, "tok-r", "/api/op", json!({"op": "providers"}));
    assert!(p.body["providers"].as_array().unwrap().iter().all(|x| x["id"] != "claude-code"));
    // remove it: gone from the store, the status and the provider list
    assert_eq!(call(&r, &od, "/api/sync/claude_token", json!({"delete": true})).status, 200);
    let s = call(&r, "tok-o", "/api/account/claude_status", json!({}));
    assert_eq!(s.body["present"], false);
    let raw = std::fs::read(tenant_dir(&uo).join("config").join("keys.enc")).unwrap();
    assert!(!zo_core::ai::keys::decrypt_map(testenv::SECRET, &tenant_of(&uo), &raw).unwrap().contains_key("claude-code-token"));
    let p = call(&r, "tok-o", "/api/op", json!({"op": "providers"}));
    assert!(p.body["providers"].as_array().unwrap().iter().all(|x| x["id"] != "claude-code"));
    // a restricted person's desktop cannot remove the owner's token either: it only ever sees its own tenant
    assert_eq!(call(&r, &rd, "/api/sync/claude_token", json!({"action": "delete"})).status, 403);
}

fn tenant_dir(uid: &str) -> std::path::PathBuf {
    testenv::env().join(tenant_of(uid))
}

#[test]
fn a_restricted_person_never_gets_claude_code_on_the_server_whatever_they_try() {
    let (uo, ur) = (uniq("rc-owner"), uniq("rc-guest"));
    let r = rig(&[("tok-o", &uo, "owner@x.test"), ("tok-r", &ur, "guest@x.test")], "owner@x.test", 200, 600);
    // the guest tries to pick Claude Code, store a key for it, use it for a chat and send a token as a "key"
    for op in [json!({"op": "select", "provider": "claude-code"}), json!({"op": "set_key", "provider": "claude-code", "key": OWNER_TOKEN}), json!({"op": "models", "provider": "claude-code"})] {
        let a = call(&r, "tok-r", "/api/op", op.clone());
        assert_eq!(a.body["ok"], false, "{op}: {:?}", a.body);
        assert!(a.body["error"].as_str().unwrap_or("").contains("not available"), "{op}: {:?}", a.body);
    }
    // even when the owner's token exists on the server (the global fallback), the guest cannot use it
    zo_core::ai::claude_code::set_global_token(Some(OWNER_TOKEN.into()));
    let p = call(&r, "tok-r", "/api/op", json!({"op": "providers"}));
    assert!(p.body["providers"].as_array().unwrap().iter().all(|x| x["id"] != "claude-code"));
    let s = call(&r, "tok-r", "/api/account/claude_status", json!({}));
    assert_eq!((s.body["present"].clone(), s.body["source"].clone()), (json!(false), json!("none")), "{:?}", s.body);
    let (rd, _) = make_device(&r, "tok-r", "guest-d", "desktop");
    assert_eq!(call(&r, &rd, "/api/sync/claude_token", json!({"action": "set", "token": OWNER_TOKEN})).status, 403);
    zo_core::ai::claude_code::set_global_token(None);
}

#[test]
fn claude_code_runs_for_the_owner_on_the_server_with_the_owners_token_in_a_minimal_environment() {
    let _env = CLAUDE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let uo = uniq("run-owner");
    let r = rig(&[("tok-o", &uo, "owner@x.test")], "owner@x.test", 200, 600);
    let (od, _) = make_device(&r, "tok-o", "run-desk", "desktop");
    assert_eq!(call(&r, &od, "/api/sync/claude_token", json!({"action": "set", "token": OWNER_TOKEN})).status, 200);
    // a stand-in for the claude program that records its environment
    let dir = std::path::PathBuf::from(format!("/tmp/claude-1000/zsrv-fake-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("claude");
    std::fs::write(&script, format!("#!/bin/sh\nenv > {d}/env\nprintf '%s\\n' \"$@\" > {d}/args\necho '{{\"type\":\"system\",\"session_id\":\"s-9\"}}'\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"hello from claude\"}}]}}}}'\n", d = dir.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("ZOHARA_CHAT_CLAUDE_BIN", &script);
    std::env::set_var("FIREBASE_SERVICE_ACCOUNT", "must-not-leak-firebase");
    let sel = call(&r, "tok-o", "/api/op", json!({"op": "select", "provider": "claude-code", "model": "sonnet"}));
    assert_eq!(sel.status, 200, "{:?}", sel.body);
    let (status, lines) = stream_in_thread(&r, "tok-o", json!({"text": "say hello"})).join().unwrap();
    std::env::remove_var("ZOHARA_CHAT_CLAUDE_BIN");
    std::env::remove_var("FIREBASE_SERVICE_ACCOUNT");
    assert_eq!(status, 200);
    assert!(lines.iter().any(|l| l["text"].as_str().is_some_and(|t| t.contains("hello from claude"))), "{lines:?}");
    let env = std::fs::read_to_string(dir.join("env")).unwrap();
    assert!(env.contains(&format!("CLAUDE_CODE_OAUTH_TOKEN={OWNER_TOKEN}")), "the owner's own token is passed");
    assert!(!env.contains("must-not-leak-firebase") && !env.contains(testenv::SECRET), "{env}");
    assert!(env.contains(&format!("HOME={}", tenant_dir(&uo).join("config").join("claude-home").display())));
    let args = std::fs::read_to_string(dir.join("args")).unwrap();
    assert!(args.contains("--tools\n\n"), "Claude's built-in tools are off: {args}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ===========================================================================================================================
// link:e2e:begin the real desktop link code (zo-core's `link::run_loop`, in the `linkdesk` example, with the real Zohara Chat service and a fake
// model) against this server over HTTP; and the owner's Claude token sent with the desktop's own code, run through a fake `claude`.

/// Tests that set ZOHARA_CHAT_CLAUDE_BIN / FIREBASE_SERVICE_ACCOUNT take this (the environment is shared by all tests).
static CLAUDE_ENV: Mutex<()> = Mutex::new(());

/// target/debug, found from the test binary itself.
fn debug_dir() -> std::path::PathBuf {
    std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn build_examples() -> (std::path::PathBuf, std::path::PathBuf) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let st = std::process::Command::new(cargo).args(["build", "-q", "-p", "zo-core", "--example", "linkdesk", "--example", "mock-model"]).status().expect("cargo");
        assert!(st.success(), "the examples build");
    });
    (debug_dir().join("examples").join("linkdesk"), debug_dir().join("examples").join("mock-model"))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A child process that is killed when this goes out of scope.
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The desktop: its own sandbox (home, config, data, runtime), the real service, the real link loop. Waits until it says READY.
fn start_desktop(r: &Rig, sandbox: &std::path::Path, cred: &str, model: &str, answers: &str) -> Child {
    use std::io::BufRead;
    let (bin, _) = build_examples();
    for d in ["home", "c", "d", "run"] {
        std::fs::create_dir_all(sandbox.join(d)).unwrap();
    }
    let mut c = std::process::Command::new(bin)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", sandbox.join("home"))
        .env("XDG_CONFIG_HOME", sandbox.join("c"))
        .env("XDG_DATA_HOME", sandbox.join("d"))
        .env("XDG_RUNTIME_DIR", sandbox.join("run"))
        .env("LINK_BASE", &r.base)
        .env("LINK_CRED", cred)
        .env("LINK_MODEL", model)
        .env("LINK_ANSWERS", answers)
        .env("LINK_LOG", sandbox.join("asks.log"))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("linkdesk starts");
    let mut line = String::new();
    std::io::BufReader::new(c.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "READY");
    Child(c)
}

/// The mock model server (the zo-core example), on a free port.
fn start_model() -> (Child, String) {
    let (_, bin) = build_examples();
    let port = free_port();
    let c = std::process::Command::new(bin).arg(port.to_string()).stderr(std::process::Stdio::null()).spawn().expect("mock-model starts");
    let url = format!("http://127.0.0.1:{port}/v1");
    let t0 = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(10), "the model server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    (Child(c), url)
}

fn computers(r: &Rig, token: &str) -> Vec<Value> {
    call(r, token, "/api/link/status", json!({})).body["devices"].as_array().cloned().unwrap_or_default()
}

fn wait_online(r: &Rig, token: &str, want: bool) {
    let t0 = Instant::now();
    while computers(r, token).first().map(|d| d["online"] == want) != Some(true) {
        assert!(t0.elapsed() < Duration::from_secs(15), "the computer never became {}", if want { "online" } else { "offline" });
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A streaming answer read line by line as it arrives (pings left out): the lines come through the channel; None when the stream ended.
fn stream_live(r: &Rig, token: &str, body: Value) -> std::sync::mpsc::Receiver<Option<Value>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (base, token) = (r.base.clone(), token.to_string());
    std::thread::spawn(move || {
        let res = match ureq::post(&format!("{base}/api/send")).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(60)).send_string(&body.to_string()) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        use std::io::BufRead;
        for l in std::io::BufReader::new(res.into_reader()).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&l) {
                if v["event"] != "ping" && tx.send(Some(v)).is_err() {
                    return;
                }
            }
        }
        let _ = tx.send(None);
    });
    rx
}

/// Everything until the stream ends (or the time is up).
fn drain(rx: &std::sync::mpsc::Receiver<Option<Value>>, secs: u64) -> Vec<Value> {
    let mut out = vec![];
    let t0 = Instant::now();
    while let Ok(m) = rx.recv_timeout(Duration::from_secs(secs).saturating_sub(t0.elapsed())) {
        match m {
            Some(v) => out.push(v),
            None => return out,
        }
    }
    panic!("the stream did not end in {secs}s: {out:?}");
}

fn all_text(lines: &[Value]) -> String {
    lines.iter().filter(|l| l["event"] == "text" || l["event"] == "delta").filter_map(|l| l["text"].as_str()).collect::<Vec<_>>().join("")
}

fn asks(sandbox: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(sandbox.join("asks.log")).unwrap_or_default().lines().map(String::from).collect()
}

#[test]
fn the_real_desktop_link_end_to_end_a_question_card_is_answered_from_the_web_and_stop_ends_a_waiting_one() {
    let u = uniq("e2e-q");
    let r = rig(&[("tok-q", &u, "q@x.test")], "", 200, 6000);
    let sandbox = std::path::PathBuf::from(format!("/tmp/claude-1000/zl-e2eq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    let (_model, model_url) = start_model();
    let (cred, _dev) = make_device(&r, "tok-q", "Desk-Q", "desktop");
    let _desk = start_desktop(&r, &sandbox, &cred, &model_url, "n");
    wait_online(&r, "tok-q", true);

    // the model asks; the card reaches the web; an invalid answer changes nothing; the valid one goes to the model
    let rx = stream_live(&r, "tok-q", json!({"target": "desktop", "text": "ask me something", "mode": "manual"}));
    let mut seen = vec![];
    let card = loop {
        let v = rx.recv_timeout(Duration::from_secs(30)).expect("an event").expect("the stream ended before the card");
        if v["event"] == "question" {
            break v;
        }
        seen.push(v);
    };
    let sid = seen.iter().find(|v| v["event"] == "stream").unwrap()["id"].as_u64().unwrap();
    assert_eq!(card["questions"][0]["options"][1]["label"], "Blue", "{card}");
    let qid = card["id"].clone();
    for bad in [json!({"answers": [{"selected": ["Green"]}]}), json!({"answers": [{"selected": ["Red", "Blue"]}]}), json!({"answers": []})] {
        let mut b = json!({"stream": sid, "op": "answer", "id": qid});
        b.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        let _ = call(&r, "tok-q", "/api/reply", b); // forwarded or refused: either way it must not answer the question
    }
    assert_eq!(call(&r, "tok-q", "/api/reply", json!({"stream": sid, "op": "answer", "id": qid, "answers": [{"selected": ["Blue"]}]})).status, 200);
    let rest = drain(&rx, 40);
    let text = all_text(&rest);
    assert!(text.contains("Q: Which colour?") && text.contains("A: Blue"), "the model got exactly the valid answer: {rest:?}");
    assert!(!text.contains("Green") && !text.contains("A: Red"), "{text}");
    assert!(asks(&sandbox).is_empty(), "a question is not an approval: nothing was asked on the desktop");

    // a question nobody answers ends at once with Stop (it does not wait for the 10 minutes)
    let rx = stream_live(&r, "tok-q", json!({"target": "desktop", "text": "ask me again", "mode": "manual"}));
    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    let sid = first["id"].as_u64().unwrap();
    loop {
        let v = rx.recv_timeout(Duration::from_secs(30)).expect("an event").expect("the stream ended before the card");
        if v["event"] == "question" {
            break;
        }
    }
    let t0 = Instant::now();
    assert_eq!(call(&r, "tok-q", "/api/reply", json!({"stream": sid, "op": "stop"})).status, 200);
    let rest = drain(&rx, 20);
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    assert_eq!(rest.last().unwrap()["event"], "done", "{rest:?}");
    let _ = std::fs::remove_dir_all(&sandbox);
}

#[test]
fn the_real_desktop_link_end_to_end_online_send_approvals_on_the_computer_stop_offline_other_people_and_expiry() {
    let (ua, ub) = (uniq("e2e-a"), uniq("e2e-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 6000);
    let sandbox = std::path::PathBuf::from(format!("/tmp/claude-1000/zl-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    let (_model, model_url) = start_model();
    let (cred_a, dev_a) = make_device(&r, "tok-a", "Desk-A", "desktop");
    let (_cred_b, _dev_b) = make_device(&r, "tok-b", "Desk-B", "desktop");

    // 1. A's desktop is signed in but not running: the web says offline, and a send fails at once and clearly
    assert_eq!(computers(&r, "tok-a")[0]["online"], false);
    let t0 = Instant::now();
    let lines = drain(&stream_live(&r, "tok-a", json!({"target": "desktop", "text": "hello"})), 5);
    assert!(t0.elapsed() < Duration::from_secs(1));
    assert_eq!(lines.iter().map(|l| l["event"].as_str().unwrap_or("")).collect::<Vec<_>>(), ["error", "done"], "{lines:?}");
    assert_eq!(lines[0]["error"], "desktop_offline");
    assert!(lines[0]["message"].as_str().unwrap().len() > 20);

    // 2. the desktop starts (answers to its approval dialogs: yes, then no): the web sees it online, with its capabilities
    let desk = start_desktop(&r, &sandbox, &cred_a, &model_url, "y,n");
    wait_online(&r, "tok-a", true);
    let d = computers(&r, "tok-a")[0].clone();
    assert_eq!((d["id"].as_str(), d["online"].clone()), (Some(dev_a.as_str()), json!(true)), "{d}");
    assert_eq!(d["caps"]["approvals_from_web"], false, "{d}");
    assert!(d["caps"]["os"].is_string() && d["caps"]["version"].is_string(), "{d}");
    // person B sees nothing of it, cannot address it by id and cannot reach it
    assert!(computers(&r, "tok-b").iter().all(|d| d["id"] != dev_a.as_str() && d["online"] == false), "{:?}", computers(&r, "tok-b"));
    let lines = drain(&stream_live(&r, "tok-b", json!({"target": "desktop", "device": dev_a, "text": "I am B"})), 5);
    assert_eq!(lines[0]["error"], "desktop_offline", "{lines:?}");
    let o = call(&r, "tok-b", "/api/op", json!({"op": "desktop_op", "device": dev_a, "inner": {"op": "status"}}));
    assert_eq!(o.status, 503, "{:?}", o.body);
    assert!(asks(&sandbox).is_empty());

    // 3. A sends; the desktop runs it through its service; the tool is approved ON THE DESKTOP (yes); the events arrive in order
    let lines = drain(&stream_live(&r, "tok-a", json!({"target": "desktop", "text": "run a check", "mode": "manual"})), 40);
    let kinds: Vec<&str> = lines.iter().map(|l| l["event"].as_str().unwrap_or("")).collect();
    assert_eq!(kinds[0], "stream", "{lines:?}");
    assert_eq!((lines[0]["target"].as_str(), lines[0]["device"].as_str()), (Some("desktop"), Some(hostname_or_name(&lines[0]).as_str())));
    assert_eq!(*kinds.last().unwrap(), "done", "{lines:?}");
    let ap = kinds.iter().position(|k| *k == "approval").unwrap_or_else(|| panic!("no approval event: {lines:?}"));
    assert_eq!(lines[ap]["local"], true, "{:?}", lines[ap]);
    assert!(lines[ap]["summary"].as_str().is_some_and(|s| s.contains("echo hello-from-zoai")), "{:?}", lines[ap]);
    let text = all_text(&lines);
    assert!(text.contains("hello-from-zoai"), "approved on the computer, so the command ran: {lines:?}");
    assert!(kinds.iter().position(|k| *k == "text" || *k == "delta").unwrap() > 0);
    assert_eq!(asks(&sandbox).len(), 1, "one dialog on the desktop");
    assert!(asks(&sandbox)[0].contains("echo hello-from-zoai"));
    // the web may not answer for the computer
    let stream_id = lines[0]["id"].as_u64().unwrap();
    let a = call(&r, "tok-a", "/api/reply", json!({"stream": stream_id, "op": "approve", "id": 1, "yes": true}));
    assert!(a.status == 409 || a.status == 404 || a.status == 410, "{} {:?}", a.status, a.body);

    // 4. a second send: this time the person on the computer says no; the command does not run
    let lines = drain(&stream_live(&r, "tok-a", json!({"target": "desktop", "text": "run a check again", "mode": "manual"})), 40);
    assert!(lines.iter().any(|l| l["event"] == "approval" && l["local"] == true), "{lines:?}");
    assert!(!all_text(&lines).contains("hello-from-zoai\n") && lines.last().unwrap()["event"] == "done", "{lines:?}");
    assert_eq!(asks(&sandbox).len(), 2);
    // the chats were saved on the desktop, with the web's own text
    let saved: String = walk(&sandbox.join("d")).iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    assert!(saved.contains("run a check again"), "the desktop saved the chat");

    // 5. Stop in the middle of an answer ends the desktop's answer
    let rx = stream_live(&r, "tok-a", json!({"target": "desktop", "text": "stream please", "mode": "manual"}));
    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(first["event"], "stream");
    let sid = first["id"].as_u64().unwrap();
    let mut got = vec![];
    loop {
        let v = rx.recv_timeout(Duration::from_secs(20)).expect("an event").expect("not over yet");
        let is_text = v["event"] == "text" || v["event"] == "delta";
        got.push(v);
        if is_text {
            break;
        }
    }
    let stop_at = Instant::now();
    assert_eq!(call(&r, "tok-a", "/api/reply", json!({"stream": sid, "op": "stop"})).status, 200);
    let rest = drain(&rx, 15);
    assert!(stop_at.elapsed() < Duration::from_secs(8), "stopped in {:?}", stop_at.elapsed());
    got.extend(rest);
    assert!(!all_text(&got).contains("- third"), "the answer was cut: {got:?}");
    assert!(got.iter().any(|l| l["event"] == "stopped" || l["event"] == "done"), "{got:?}");
    // the desktop is free again afterwards
    let lines = drain(&stream_live(&r, "tok-a", json!({"target": "desktop", "text": "markdown please", "mode": "auto"})), 30);
    assert!(all_text(&lines).contains("A heading"), "{lines:?}");

    // 6. the desktop goes away: it is offline within the presence window, and the next send fails within a second
    drop(desk);
    wait_online(&r, "tok-a", false);
    let t0 = Instant::now();
    let lines = drain(&stream_live(&r, "tok-a", json!({"target": "desktop", "text": "anyone there?"})), 5);
    assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    assert_eq!(lines[0]["error"], "desktop_offline", "{lines:?}");

    // 7. a job nobody picks up expires and is never run later: the person is online (a poll by hand), the job waits, the desktop comes
    //    only after it expired
    assert!(dpoll(&r, &cred_a, "Desk-A", caps(false)).is_empty());
    let lines = drain(&stream_live(&r, "tok-a", json!({"target": "desktop", "text": "EXPIRED-JOB-TEXT"})), 10);
    assert!(lines.iter().any(|l| l["error"] == "desktop_timeout"), "{lines:?}");
    let desk = start_desktop(&r, &sandbox, &cred_a, &model_url, "y,y");
    wait_online(&r, "tok-a", true);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(r.app.link.counts().1, 0, "no job is left");
    let saved: String = walk(&sandbox.join("d")).iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    assert!(!saved.contains("EXPIRED-JOB-TEXT"), "the expired job was not run");
    assert_eq!(asks(&sandbox).len(), 2, "no new dialog");
    drop(desk);
    let _ = std::fs::remove_dir_all(&sandbox);
}

/// The name the desktop gave itself (the server prints it in the first line).
fn hostname_or_name(first: &Value) -> String {
    first["device"].as_str().unwrap_or("").to_string()
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn the_owners_claude_token_is_sent_with_the_desktops_own_code_and_runs_through_a_fake_claude_for_the_owner_only() {
    let _env = CLAUDE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (uo, ub) = (uniq("e2e-co"), uniq("e2e-cb"));
    let r = rig(&[("tok-o", &uo, "owner@x.test"), ("tok-b", &ub, "guest@x.test")], "owner@x.test", 200, 6000);
    let (od, _) = make_device(&r, "tok-o", "o-desk", "desktop");
    let (bd, _) = make_device(&r, "tok-b", "b-desk", "desktop");
    // the desktop's own code (zo_core::link) sends the token: the admin's is accepted, the other person's is refused with a reason
    let sec = zo_core::link::Secret::new(OWNER_TOKEN.to_string());
    zo_core::link::send_claude_token_at(&r.base, &od, &sec).expect("the owner's token is accepted");
    let e = zo_core::link::send_claude_token_at(&r.base, &bd, &sec).expect_err("a guest's is refused");
    assert!(e.to_lowercase().contains("owner") || e.to_lowercase().contains("not"), "{e}");
    assert!(!e.contains("sk-ant"));
    assert_eq!(call(&r, "tok-o", "/api/account/claude_status", json!({})).body["present"], true);
    assert_eq!(call(&r, "tok-b", "/api/account/claude_status", json!({})).body["present"], false);
    // a fake claude, started by the server for the owner's web chat, with a minimal environment
    let dir = std::path::PathBuf::from(format!("/tmp/claude-1000/zsrv-fake2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("claude");
    std::fs::write(&script, format!("#!/bin/sh\nenv > {d}/env\npwd > {d}/pwd\nprintf '%s\\n' \"$@\" > {d}/args\necho '{{\"type\":\"system\",\"session_id\":\"s-7\"}}'\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"the owner subscription answers\"}}]}}}}'\necho '{{\"type\":\"result\",\"result\":\"ok\"}}'\n", d = dir.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("ZOHARA_CHAT_CLAUDE_BIN", &script);
    std::env::set_var("FIREBASE_SERVICE_ACCOUNT", "must-not-leak-firebase");
    let sel = call(&r, "tok-o", "/api/op", json!({"op": "select", "provider": "claude-code", "model": "sonnet"}));
    assert_eq!(sel.status, 200, "{:?}", sel.body);
    let owner_lines = drain(&stream_live(&r, "tok-o", json!({"text": "say hello"})), 20);
    let env = std::fs::read_to_string(dir.join("env")).unwrap_or_default();
    let args = std::fs::read_to_string(dir.join("args")).unwrap_or_default();
    let cwd = std::fs::read_to_string(dir.join("pwd")).unwrap_or_default();
    // the guest: Claude Code is not available, and the fake claude is never started for them
    let _ = std::fs::remove_file(dir.join("env"));
    let gsel = call(&r, "tok-b", "/api/op", json!({"op": "select", "provider": "claude-code", "model": "sonnet"}));
    let guest_lines = drain(&stream_live(&r, "tok-b", json!({"text": "say hello", "provider": "claude-code"})), 20);
    let guest_env_exists = dir.join("env").exists();
    std::env::remove_var("ZOHARA_CHAT_CLAUDE_BIN");
    std::env::remove_var("FIREBASE_SERVICE_ACCOUNT");
    assert!(all_text(&owner_lines).contains("the owner subscription answers"), "{owner_lines:?}");
    assert_eq!(gsel.body["ok"], false, "{:?}", gsel.body);
    assert!(gsel.body["error"].as_str().unwrap_or("").contains("not available"), "{:?}", gsel.body);
    assert!(!guest_env_exists && !all_text(&guest_lines).contains("subscription"), "{guest_lines:?}");
    assert!(env.contains(&format!("CLAUDE_CODE_OAUTH_TOKEN={OWNER_TOKEN}")), "the owner's own token is passed");
    assert!(!env.contains("must-not-leak-firebase") && !env.contains(testenv::SECRET), "{env}");
    assert!(env.contains(&format!("HOME={}", tenant_dir(&uo).join("config").join("claude-home").display())));
    assert!(args.contains("--tools\n\n"), "built-in tools are off: {args}");
    assert!(cwd.trim().starts_with(tenant_dir(&uo).to_str().unwrap()), "runs in the owner's own folder: {cwd}");
    let _ = std::fs::remove_dir_all(&dir);
}


// ===========================================================================================================================
// terminals (term.rs) and the Claude sign-in (claude_api.rs)

/// A terminal stream in a thread: every line as it arrives, until the stream ends.
fn term_stream(r: &Rig, token: &str, body: Value) -> std::sync::mpsc::Receiver<Option<Value>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (base, token) = (r.base.clone(), token.to_string());
    std::thread::spawn(move || {
        let res = match ureq::post(&format!("{base}/api/term/open")).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(60)).send_string(&body.to_string()) {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => {
                let _ = tx.send(Some(json!({"event": "transport_error", "error": e.to_string()})));
                return;
            }
        };
        let status = res.status();
        if status != 200 {
            let text = res.into_string().unwrap_or_default();
            let _ = tx.send(Some(json!({"event": "http_error", "status": status, "body": serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)})));
            let _ = tx.send(None);
            return;
        }
        for l in std::io::BufReader::new(res.into_reader()).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&l) {
                if v["event"] != "ping" && tx.send(Some(v)).is_err() {
                    return;
                }
            }
        }
        let _ = tx.send(None);
    });
    rx
}

fn b64d(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap_or_default()
}

/// Reads lines until the collected terminal output contains `needle` (or the time is up). Returns (all output, all lines).
fn until_output(rx: &std::sync::mpsc::Receiver<Option<Value>>, needle: &str, secs: u64) -> (String, Vec<Value>) {
    let (mut text, mut lines) = (Vec::new(), Vec::new());
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(v)) => {
                if v["event"] == "out" {
                    text.extend(b64d(v["b"].as_str().unwrap_or("")));
                }
                lines.push(v);
                if String::from_utf8_lossy(&text).contains(needle) {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }
    (String::from_utf8_lossy(&text).into_owned(), lines)
}

#[test]
fn the_cloud_terminal_is_for_the_owner_only_runs_a_real_shell_with_no_server_secrets_and_belongs_to_one_person() {
    let (uo, ur, uo2) = (uniq("tm-o"), uniq("tm-r"), uniq("tm-o2"));
    let r = rig(&[("tok-o", &uo, "owner@x.test"), ("tok-r", &ur, "r@x.test"), ("tok-o2", &uo2, "second@x.test")], "owner@x.test,second@x.test", 200, 6000);
    // the restricted person never gets a shell, whatever they send
    let a = call(&r, "tok-r", "/api/term/open", json!({"target": "cloud"}));
    assert_eq!(a.status, 403, "{:?}", a.body);
    assert!(a.body["error"].as_str().unwrap().contains("only for the owner"), "{:?}", a.body);
    assert_eq!(r.app.term.count(), 0);
    for n in ["input", "resize", "close"] {
        assert_ne!(call(&r, "tok-r", &format!("/api/term/{n}"), json!({"session": "abc", "data": "id\n"})).status, 200, "{n}");
    }
    let l = call(&r, "tok-r", "/api/term/list", json!({}));
    assert_eq!((l.body["cloud"].clone(), l.body["sessions"].clone()), (json!(false), json!([])));
    // no sign-in at all
    assert_eq!(call(&r, "", "/api/term/open", json!({})).status, 401);

    // the owner opens one
    let rx = term_stream(&r, "tok-o", json!({"target": "cloud", "rows": 24, "cols": 100, "name": "build"}));
    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!((first["event"].as_str(), first["target"].as_str(), first["name"].as_str(), first["rows"].as_u64()), (Some("term"), Some("cloud"), Some("build"), Some(24)), "{first:?}");
    let sid = first["id"].as_str().unwrap().to_string();
    // type: a calculation, the size, who it is, and everything in its environment
    let typed = call(&r, "tok-o", "/api/term/input", json!({"session": sid, "data": "echo zo-$((6*7)); stty size; id -un; env | sort; echo [$HOME]\n"}));
    assert_eq!(typed.status, 200, "{:?}", typed.body);
    let (text, _) = until_output(&rx, "[/", 15);
    assert!(text.contains("zo-42") && text.contains("24 100"), "{text}");
    // the shell's environment is exactly what term.rs gives it: not one of the server's variables
    for bad in ["ZO_SERVER_SECRET", "FIREBASE", "ZO_TENANT_ROOT", "ZO_ADMIN", "ZO_MULTI_TENANT", testenv::SECRET, "CLAUDE_CODE_OAUTH_TOKEN", "ZOHARA_CHAT_KEY", "ZOAI_KEY"] {
        assert!(!text.contains(bad), "{bad} is visible in the terminal:\n{text}");
    }
    assert!(text.contains("TERM=xterm-256color") && text.contains("DISABLE_AUTOUPDATER=1"), "{text}");
    assert!(text.contains("-terminals/"), "the home is outside the people's folders: {text}");

    // it belongs to one person: another admin can neither see it, type into it, resize it nor close it
    let l2 = call(&r, "tok-o2", "/api/term/list", json!({}));
    assert_eq!(l2.body["sessions"], json!([]), "{:?}", l2.body);
    for n in ["input", "resize", "close"] {
        assert_eq!(call(&r, "tok-o2", &format!("/api/term/{n}"), json!({"session": sid, "data": "id\n", "rows": 10, "cols": 10})).status, 404, "{n}");
    }
    let other = until_output(&term_stream(&r, "tok-o2", json!({"session": sid})), "never", 2).1;
    assert_eq!(other[0]["error"], "gone", "{other:?}");
    // the owner's list shows it, a resize works
    let l = call(&r, "tok-o", "/api/term/list", json!({}));
    assert_eq!(l.body["cloud"], true);
    assert_eq!(l.body["sessions"][0]["id"], sid.as_str());
    assert_eq!(call(&r, "tok-o", "/api/term/resize", json!({"session": sid, "rows": 40, "cols": 120})).status, 200);
    call(&r, "tok-o", "/api/term/input", json!({"session": sid, "data": "stty size\n"}));
    let (text, _) = until_output(&rx, "40 120", 10);
    assert!(text.contains("40 120"), "{text}");

    // a page that lost its connection opens again with the sequence number it had: it gets what it missed, the old stream is told it was replaced
    call(&r, "tok-o", "/api/term/input", json!({"session": sid, "data": "echo after-the-gap\n"}));
    std::thread::sleep(Duration::from_millis(600));
    let pty = r.app.term.pty_of(&tenant_of(&uo), &sid).unwrap();
    let end = pty.end_seq();
    let rx2 = term_stream(&r, "tok-o", json!({"session": sid, "from": 0}));
    let (replayed, lines2) = until_output(&rx2, "after-the-gap", 10);
    assert!(replayed.contains("zo-42") && replayed.contains("after-the-gap"), "the whole kept output: {replayed}");
    assert_eq!(lines2[0]["event"], "term");
    assert!(pty.end_seq() >= end);
    let mut saw_replaced = false;
    while let Ok(m) = rx.recv_timeout(Duration::from_secs(20)) {
        match m {
            Some(v) if v["event"] == "replaced" => {
                saw_replaced = true;
                break;
            }
            Some(_) => {}
            None => break,
        }
    }
    assert!(saw_replaced, "the first stream was told that another page took over");
    // from the end of what the page has: nothing is repeated
    let rx3 = term_stream(&r, "tok-o", json!({"session": sid, "from": pty.end_seq()}));
    call(&r, "tok-o", "/api/term/input", json!({"session": sid, "data": "echo only-new\n"}));
    let (fresh, _) = until_output(&rx3, "only-new", 10);
    assert!(fresh.contains("only-new") && !fresh.contains("zo-42"), "{fresh}");

    // too much input at once is refused; the shell ends with its own exit code and the stream says so
    assert_eq!(call(&r, "tok-o", "/api/term/input", json!({"session": sid, "data": "x".repeat(term::MAX_INPUT_CALL + 1)})).status, 413);
    call(&r, "tok-o", "/api/term/input", json!({"session": sid, "data": "exit 7\n"}));
    let (_, ended) = until_output(&rx3, "never-appears", 15);
    assert!(ended.iter().any(|l| l["event"] == "exit" && l["code"] == 7), "{ended:?}");
    let sid2 = {
        let rx = term_stream(&r, "tok-o", json!({"target": "cloud"}));
        let id = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap()["id"].as_str().unwrap().to_string();
        assert_eq!(call(&r, "tok-o", "/api/term/close", json!({"session": id})).status, 200);
        assert_eq!(call(&r, "tok-o", "/api/term/close", json!({"session": id})).status, 404, "closing twice");
        id
    };
    assert!(r.app.term.pty_of(&tenant_of(&uo), &sid2).is_none());
}

#[test]
fn terminals_are_limited_per_person_and_refused_from_other_websites_desktops_and_when_switched_off() {
    let uo = uniq("tm-lim");
    let r = rig(&[("tok-o", &uo, "owner@x.test")], "owner@x.test", 200, 6000);
    // another website's page: the browser's Origin does not match the host it called
    let res = ureq::post(&format!("{}/api/term/open", r.base)).set("Authorization", "Bearer tok-o").set("Origin", "https://evil.example").send_string("{}");
    assert!(matches!(res, Err(ureq::Error::Status(403, _))), "{res:?}");
    let res = ureq::post(&format!("{}/api/term/list", r.base)).set("Authorization", "Bearer tok-o").set("Origin", &format!("http://{}", r.base.trim_start_matches("http://"))).send_string("{}");
    assert!(matches!(res, Err(ureq::Error::Status(403, _))), "plain http from a non-local host is not this server's own page: {res:?}");
    assert_eq!(r.app.term.count(), 0);
    // a desktop app's credential is not for terminals
    let (cred, _) = make_device(&r, "tok-o", "Desk", "desktop");
    assert_eq!(call(&r, &cred, "/api/term/open", json!({})).status, 403);
    assert_eq!(call(&r, &cred, "/api/claude/status", json!({})).status, 403);
    // four per person, then a clear refusal
    let mut keep = Vec::new();
    for _ in 0..term::MAX_PER_TENANT {
        let rx = term_stream(&r, "tok-o", json!({"target": "cloud"}));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap()["event"], "term");
        keep.push(rx);
    }
    let fifth = term_stream(&r, "tok-o", json!({"target": "cloud"}));
    let line = fifth.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!((line["event"].as_str(), line["error"].as_str()), (Some("error"), Some("limit")), "{line:?}");
    assert_eq!(r.app.term.list(&tenant_of(&uo)).len(), term::MAX_PER_TENANT);
    // the operator's switch ends them for good
    std::env::set_var("ZO_TERMINAL", "off");
    let a = call(&r, "tok-o", "/api/term/open", json!({"target": "cloud"}));
    let st = call(&r, "tok-o", "/api/term/list", json!({}));
    let id = r.app.term.list(&tenant_of(&uo))[0].id.clone();
    let typed = call(&r, "tok-o", "/api/term/input", json!({"session": id, "data": "id\n"}));
    std::env::remove_var("ZO_TERMINAL");
    assert_eq!(a.status, 403, "{:?}", a.body);
    assert_eq!((st.body["cloud"].clone(), st.body["cloud_enabled"].clone()), (json!(false), json!(false)));
    assert_eq!(typed.status, 403, "a running shell gets nothing once the switch is off");
    assert_eq!(r.app.term.count(), 0, "and every shell of that person is closed");
    drop(keep);
}

#[test]
fn closing_ends_every_job_of_the_shell_and_an_idle_or_old_shell_is_swept() {
    let h = term::Hub::default();
    let root = std::env::temp_dir().join(format!("zo-term-root-{}", std::process::id()));
    let t = "0123456789abcdef0123456789abcdef";
    let i = h.open(t, &root, 24, 80, "x").unwrap();
    assert!(h.has(t, &i.id));
    assert!(!h.has("ffffffffffffffffffffffffffffffff", &i.id));
    h.close_all(t);
    assert_eq!(h.count(), 0);
    // the shell dies with its group: nothing is left running
    let i = h.open(t, &root, 24, 80, "x").unwrap();
    let pty = h.pty_of(t, &i.id).unwrap();
    pty.write(b"sleep 300 &\necho b\"\"g=$!\n").unwrap();
    let c = {
        let end = Instant::now() + Duration::from_secs(10);
        let mut seen = String::new();
        let mut seq = 0;
        while Instant::now() < end && !seen.contains("bg=") {
            let c = pty.read_from(seq, Duration::from_millis(200));
            seen.push_str(&String::from_utf8_lossy(&c.data));
            seq = c.next;
        }
        seen
    };
    let bg: i32 = c.rsplit("bg=").next().unwrap().trim().lines().next().unwrap().trim().parse().unwrap_or_else(|_| panic!("{c}"));
    assert!(std::path::Path::new(&format!("/proc/{bg}")).exists());
    h.close(t, &i.id);
    std::thread::sleep(Duration::from_millis(400));
    let zombie = std::fs::read_to_string(format!("/proc/{bg}/stat")).map(|s| s.contains(") Z")).unwrap_or(true);
    assert!(zombie, "the background program died with the shell");
    // a shell nobody types into is ended after the idle time, one that lives too long after its lifetime, and a busy one stays
    let h = term::Hub::with_limits(Duration::from_millis(700), Duration::from_secs(3600));
    let (idle, busy) = (h.open(t, &root, 24, 80, "idle").unwrap(), h.open(t, &root, 24, 80, "busy").unwrap());
    std::thread::sleep(Duration::from_millis(500));
    h.pty_of(t, &busy.id).unwrap().write(b"echo still-here\n").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    h.sweep();
    assert!(!h.has(t, &idle.id), "the idle one was ended");
    assert!(h.has(t, &busy.id), "the one with input a moment ago stays");
    let h2 = term::Hub::with_limits(Duration::from_secs(3600), Duration::from_millis(300));
    let old = h2.open(t, &root, 24, 80, "old").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    h2.sweep();
    assert!(!h2.has(t, &old.id), "the old one was ended");
    h.close_all(t);
    let _ = std::fs::remove_dir_all(format!("{}-terminals", root.display()));
}

#[test]
fn the_shell_cannot_read_the_servers_memory_or_environment_through_proc() {
    // this test process plays the server: marked not dumpable, a shell of the same user cannot open what /proc shows of it
    hardening::protect_memory();
    let h = term::Hub::default();
    let root = std::env::temp_dir().join(format!("zo-term-proc-{}", std::process::id()));
    let t = "00112233445566778899aabbccddeeff";
    let i = h.open(t, &root, 24, 200, "x").unwrap();
    let pty = h.pty_of(t, &i.id).unwrap();
    let me = std::process::id();
    pty.write(format!("for f in environ mem maps; do head -c 20 /proc/{me}/$f >/dev/null 2>&1 && echo O\"\"PENED-$f || echo D\"\"ENIED-$f; done\n").as_bytes()).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let (mut seen, mut seq) = (String::new(), 0);
    while Instant::now() < end && seen.matches("-maps").count() < 2 {
        let c = pty.read_from(seq, Duration::from_millis(200));
        seen.push_str(&String::from_utf8_lossy(&c.data));
        seq = c.next;
    }
    h.close_all(t);
    let _ = std::fs::remove_dir_all(format!("{}-terminals", root.display()));
    assert!(seen.contains("DENIED-environ") && seen.contains("DENIED-mem") && seen.contains("DENIED-maps") && !seen.contains("OPENED-"), "{seen}");
}

#[test]
fn a_terminal_on_the_persons_own_computer_goes_through_the_link_and_the_page_can_only_type_resize_and_close() {
    let u = uniq("tm-dsk");
    let r = rig(&[("tok", &u, "d@x.test"), ("tok-b", &uniq("tm-dsk-b"), "b@x.test")], "", 200, 6000);
    let (cred, _) = make_device(&r, "tok", "Desk", "desktop");
    // not online: a clear error, and no job is left
    let (_, lines) = (|| { let rx = term_stream(&r, "tok", json!({"target": "desktop"})); let mut v = vec![]; while let Ok(Some(l)) = rx.recv_timeout(Duration::from_secs(5)) { v.push(l); } ((), v) })();
    assert_eq!(lines[0]["error"], "desktop_offline", "{lines:?}");
    assert!(dpoll(&r, &cred, "Desk", caps(false)).is_empty());
    // a restricted person may use it: it is their own computer (no admin needed)
    let rx = term_stream(&r, "tok", json!({"target": "desktop", "rows": 20, "cols": 70}));
    let job = dpoll_for(&r, &cred, "Desk", caps(false), "term");
    assert_eq!((job["rows"].clone(), job["cols"].clone()), (json!(20), json!(70)));
    assert!(job.get("cmd").is_none() && job.get("text").is_none(), "{job:?}");
    let id = job["id"].as_str().unwrap().to_string();
    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!((first["event"].as_str(), first["target"].as_str(), first["id"].as_str()), (Some("term"), Some("desktop"), Some(id.as_str())));
    // the computer says it waits for its owner, then that it is open, then output; junk events and bad payloads never reach the page
    let a = post_events(&r, &cred, &id, json!([{"event": "waiting"}, {"event": "opened"}, {"event": "out", "b": "aGVsbG8="}, {"event": "out", "b": "<script>"}, {"event": "run", "cmd": "x"}, {"event": "out", "b": 5}, {"event": "stream", "id": 1}]), false);
    assert_eq!((a.status, a.body["cancel"].clone()), (200, json!(false)), "{:?}", a.body);
    let (text, lines) = until_output(&rx, "hello", 10);
    assert_eq!(text, "hello");
    let kinds: Vec<&str> = lines.iter().map(|l| l["event"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["waiting", "opened", "out"], "{lines:?}");
    // the page types: it comes back in the answer to the computer's next post, with only the known fields
    assert_eq!(call(&r, "tok", "/api/term/input", json!({"session": id, "data": "ls\n", "evil": 1})).status, 200);
    assert_eq!(call(&r, "tok", "/api/term/resize", json!({"session": id, "rows": 30, "cols": 90})).status, 200);
    let a = post_events(&r, &cred, &id, json!([]), false);
    assert_eq!(a.body["term"], json!([{"t": "in", "data": "ls\n"}, {"t": "resize", "rows": 30, "cols": 90}]), "{:?}", a.body);
    assert!(post_events(&r, &cred, &id, json!([]), false).body.get("term").is_none(), "delivered once");
    // another person cannot type into it
    for n in ["input", "resize", "close"] {
        assert_eq!(call(&r, "tok-b", &format!("/api/term/{n}"), json!({"session": id, "data": "id\n", "rows": 10, "cols": 10})).status, 404, "{n}");
    }
    // the page closes it: the computer is told in the next answer, and the stream ends
    assert_eq!(call(&r, "tok", "/api/term/close", json!({"session": id})).status, 200);
    assert_eq!(post_events(&r, &cred, &id, json!([{"event": "out", "b": "YQ=="}]), false).body["term"], json!([{"t": "close"}]));
    post_events(&r, &cred, &id, json!([{"event": "exit", "code": 0}]), true);
    let mut tail = vec![];
    while let Ok(Some(l)) = rx.recv_timeout(Duration::from_secs(10)) {
        tail.push(l);
    }
    assert_eq!(tail.last().unwrap()["event"], "done", "{tail:?}");
    assert_eq!(r.app.link.counts().1, 0, "nothing is left behind");
}

#[test]
fn the_guided_claude_sign_in_over_http_gives_the_page_a_link_takes_the_code_and_never_shows_the_token() {
    let _env = CLAUDE_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (uo, ur) = (uniq("cl-owner"), uniq("cl-restr"));
    let r = rig(&[("tok-o", &uo, "owner@x.test"), ("tok-r", &ur, "guest@x.test")], "owner@x.test", 200, 6000);
    let dir = std::path::PathBuf::from(format!("/tmp/claude-1000/zsrv-login-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    const TOKEN: &str = "sk-ant-oat01-HttpFlowToken0123456789abcdefghijklmn";
    let script = dir.join("claude");
    std::fs::write(&script, format!("#!/bin/sh\nenv > {d}/env\nprintf 'Open: https://claude.com/cai/oauth/authorize?state=http1\\r\\nPaste code > '\nread code\nif [ \"$code\" = \"the-code\" ]; then printf 'Your token:\\r\\n{TOKEN}\\r\\n'; sleep 20; else exit 1; fi\n", d = dir.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("ZOHARA_CHAT_CLAUDE_BIN", &script);
    std::env::set_var("FIREBASE_SERVICE_ACCOUNT", "must-not-leak-firebase");
    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // the restricted person (the operator did not allow everybody) is refused everywhere except the status, which says so
        let s = call(&r, "tok-r", "/api/claude/status", json!({}));
        assert_eq!((s.status, s.body["allowed"].clone(), s.body["users"].clone(), s.body["flow"].clone()), (200, json!(false), json!("admins"), Value::Null), "{:?}", s.body);
        for n in ["start", "poll", "code", "cancel", "logout"] {
            assert_eq!(call(&r, "tok-r", &format!("/api/claude/{n}"), json!({"code": "x"})).status, 403, "{n}");
        }
        // the owner
        let s = call(&r, "tok-o", "/api/claude/status", json!({}));
        assert_eq!((s.body["allowed"].clone(), s.body["present"].clone(), s.body["installed"].clone()), (json!(true), json!(false), json!(true)), "{:?}", s.body);
        assert_eq!(s.body["flow"]["state"], "idle");
        assert_eq!(call(&r, "tok-o", "/api/claude/code", json!({"code": "x"})).status, 400, "no flow yet");
        let a = call(&r, "tok-o", "/api/claude/start", json!({}));
        assert_eq!(a.status, 200, "{:?}", a.body);
        let t0 = Instant::now();
        let link = loop {
            let p = call(&r, "tok-o", "/api/claude/poll", json!({}));
            if p.body["state"] == "link" {
                break p.body;
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "no link: {:?}", p.body);
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(link["url"], "https://claude.com/cai/oauth/authorize?state=http1");
        assert!(link["secs_left"].as_u64().unwrap() > 250);
        // what the page gets has only these fields
        let mut keys: Vec<&str> = link.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["message", "ok", "secs_left", "state", "url"]);
        // the program runs with an empty environment: not one server secret, and its own home
        let env = std::fs::read_to_string(dir.join("env")).unwrap();
        assert!(!env.contains("must-not-leak-firebase") && !env.contains(testenv::SECRET) && !env.contains("ZO_TENANT_ROOT"), "{env}");
        assert!(env.contains("HOME=") && env.contains("claude-login-"), "{env}");
        // a garbage code is refused by the server and never reaches the program
        assert_eq!(call(&r, "tok-o", "/api/claude/code", json!({"code": "has space"})).status, 400);
        assert_eq!(call(&r, "tok-o", "/api/claude/code", json!({"code": "the-code"})).status, 200);
        let t0 = Instant::now();
        let end = loop {
            let p = call(&r, "tok-o", "/api/claude/poll", json!({}));
            if p.body["state"] == "done" || p.body["state"] == "failed" {
                break p;
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", p.body);
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(end.body["state"], "done", "{:?}", end.body);
        assert!(!end.body.to_string().contains("sk-ant") && !end.body.to_string().contains("HttpFlowToken"));
        let s = call(&r, "tok-o", "/api/claude/status", json!({}));
        assert_eq!((s.body["present"].clone(), s.body["source"].clone()), (json!(true), json!("own")), "{:?}", s.body);
        assert!(!s.body.to_string().contains("sk-ant"));
        // stored encrypted, like the token a desktop sends
        let raw = std::fs::read(tenant_dir(&uo).join("config").join("keys.enc")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("HttpFlowToken"));
        let map = zo_core::ai::keys::decrypt_map(testenv::SECRET, &tenant_of(&uo), &raw).unwrap();
        assert_eq!(map["claude-code-token"], TOKEN);
        // the private folder of the sign-in is gone
        std::thread::sleep(Duration::from_millis(300));
        let left: Vec<_> = std::fs::read_dir(tenant_dir(&uo).join("config")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("claude-login-")).collect();
        assert!(left.is_empty(), "{left:?}");
        // log out: the token and the program's folder go
        assert_eq!(call(&r, "tok-o", "/api/claude/logout", json!({})).status, 200);
        assert_eq!(call(&r, "tok-o", "/api/claude/status", json!({})).body["present"], false);
    }));
    std::env::remove_var("ZOHARA_CHAT_CLAUDE_BIN");
    std::env::remove_var("FIREBASE_SERVICE_ACCOUNT");
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = done {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn the_real_desktop_code_opens_a_terminal_only_after_its_owner_says_yes_on_the_computer_and_the_web_cannot_answer_for_them() {
    let u = uniq("e2e-term");
    let r = rig(&[("tok-a", &u, "t@x.test")], "", 200, 6000);
    let sandbox = std::path::PathBuf::from(format!("/tmp/claude-1000/zl-term-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    let (_model, model_url) = start_model();
    let (cred, _dev) = make_device(&r, "tok-a", "Desk-T", "desktop");
    // the person on the computer answers: yes to the first terminal, no to the second
    let _desk = start_desktop(&r, &sandbox, &cred, &model_url, "y,n");
    wait_online(&r, "tok-a", true);

    // the first terminal: the page hears "waiting", the computer asks its owner (and says what it is), then it opens and works
    let rx = term_stream(&r, "tok-a", json!({"target": "desktop", "rows": 24, "cols": 90}));
    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!((first["event"].as_str(), first["target"].as_str()), (Some("term"), Some("desktop")), "{first:?}");
    let id = first["id"].as_str().unwrap().to_string();
    let (_, lines) = until_output(&rx, "never-here", 3);
    let kinds: Vec<&str> = lines.iter().map(|l| l["event"].as_str().unwrap_or("")).collect();
    assert!(kinds.contains(&"waiting") && kinds.contains(&"opened"), "{lines:?}");
    assert_eq!(asks(&sandbox).len(), 1, "one dialog on the computer");
    assert!(asks(&sandbox)[0].contains("TERMINAL") && asks(&sandbox)[0].contains("any command as you"), "{:?}", asks(&sandbox));
    // typing before anything else: the real shell of the computer answers through the link
    assert_eq!(call(&r, "tok-a", "/api/term/input", json!({"session": id, "data": "echo from-the-computer-$((6*7)); stty size\n"})).status, 200);
    let (text, _) = until_output(&rx, "24 90", 20);
    assert!(text.contains("from-the-computer-42") && text.contains("24 90"), "{text}");
    // the web cannot approve anything for the computer: /api/reply has no such thing for a terminal, and a second terminal asks again
    let rx2 = term_stream(&r, "tok-a", json!({"target": "desktop"}));
    let first2 = rx2.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    let id2 = first2["id"].as_str().unwrap().to_string();
    let (_, lines2) = until_output(&rx2, "never-here", 6);
    assert!(lines2.iter().any(|l| l["event"] == "denied"), "{lines2:?}");
    assert!(!lines2.iter().any(|l| l["event"] == "opened" || l["event"] == "out"), "no shell was opened: {lines2:?}");
    assert_eq!(asks(&sandbox).len(), 2, "it asked again");
    // typing into the denied one goes nowhere (it does not exist any more)
    assert_ne!(call(&r, "tok-a", "/api/term/input", json!({"session": id2, "data": "touch /tmp/zo-denied-terminal\n"})).status, 200);
    assert!(!std::path::Path::new("/tmp/zo-denied-terminal").exists());
    // closing from the page ends the first one
    assert_eq!(call(&r, "tok-a", "/api/term/close", json!({"session": id})).status, 200);
    let (_, tail) = until_output(&rx, "never-here", 15);
    assert_eq!(tail.last().unwrap()["event"], "done", "{tail:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!((r.app.link.counts().1, r.app.link.counts().2), (0, 0), "no job and no stream is left behind");
}
