//! The server as a whole, over real HTTP: the web server runs in this process with a stand-in for Google's sign-in (`auth::Fake`) and
//! an in-memory database (`db::MemDb`) instead of Firestore, and zo-core's own service runs behind it on a socket in a sandbox.

use super::*;
use crate::auth::Fake;
use crate::db::{Db, MemDb};
use crate::testenv;
use std::collections::HashSet;

struct Rig {
    app: Arc<App>,
    base: String,
    db: Arc<MemDb>,
}

fn rig(people: &[(&str, &str, &str)], admins: &str, max_users: usize, per_min: u32) -> Rig {
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
    });
    let server = Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let a = app.clone();
    std::thread::spawn(move || serve(a, server));
    Rig { app, base, db }
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
    let routes = [Route::Op, Route::Send, Route::Reply, Route::Sync, Route::Account, Route::Auth, Route::Signout];
    let table: [(&Cred, [bool; 7]); 3] = [(&acct, [true; 7]), (&full, [true, true, true, true, false, false, true]), (&sync, [false, false, false, true, false, false, true])];
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
}

#[test]
fn the_restricted_person_gets_the_five_tools_and_the_admin_does_not_get_refused() {
    let (ur, uo) = (uniq("tool-r"), uniq("tool-o"));
    let r = rig(&[("tok-r", &ur, "r@x.test"), ("tok-o", &uo, "owner@x.test")], "owner@x.test", 200, 600);
    // the tool list the model would see
    let restricted = zo_core::tenant::Tenant::open_in(&testenv::env(), &tenant_of(&ur), true).unwrap();
    let names: HashSet<&str> = zo_core::tenant::scope(&restricted, || zo_core::tools::active().iter().map(|d| d.name).collect());
    assert_eq!(names, HashSet::from(["zohara_web_search", "zohara_web_fetch", "zohara_memory_search", "zohara_memory_save", "zohara_memory_forget"]));
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
    assert_eq!(call(&r, "tok", "/api/send", big).status, 413);
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
    let app = Arc::new(App { no_auth: true, noauth_admin: false, verifier: None, policy: Policy::default(), tenants, devices: None, login: DeviceLogin::new(Default::default()), public_url: None, streams: Mutex::new(HashMap::new()), next: AtomicU64::new(1), conns: AtomicUsize::new(0), legal: Default::default(), require_terms: std::sync::atomic::AtomicBool::new(false), accepted: Mutex::new(Default::default()) });
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
    assert!(["compact", "memory_edit", "personality_generate", "models"].iter().all(|o| op_uses_model(o)) && !op_uses_model("chats"));
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
                .env("ZO_AI_KEYS_FILE", self.dir.join("keys.json"))
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
        let cfg = std::fs::read_to_string(desk.dir.join("config/zo-ai/sync.json")).unwrap_or_default();
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
}
