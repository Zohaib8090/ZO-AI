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
    let full = Cred::Device { scope: Scope::Full };
    let sync = Cred::Device { scope: Scope::Sync };
    let routes = [Route::Op, Route::Send, Route::Reply, Route::Sync, Route::Account];
    let table: [(&Cred, [bool; 5]); 3] = [(&acct, [true, true, true, true, true]), (&full, [true, true, true, true, false]), (&sync, [false, false, false, true, false])];
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
    for p in ["/api/account/", "/api/account/other", "/api/sync/other", "/api/sync", "/api/opx", "/", "/healthz"] {
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
fn device_tokens_a_sync_token_cannot_call_the_api_a_full_one_can_a_revoked_one_stops() {
    let u = uniq("dev");
    let r = rig(&[("tok", &u, "d@x.test")], "", 200, 600);
    let sync = call(&r, "tok", "/api/account/device_create", json!({"name": "laptop", "scope": "sync"}));
    let full = call(&r, "tok", "/api/account/device_create", json!({"name": "terminal", "scope": "full"}));
    assert_eq!((sync.status, full.status), (200, 200), "{:?}", sync.body);
    let (st, ft) = (sync.body["token"].as_str().unwrap().to_string(), full.body["token"].as_str().unwrap().to_string());
    assert!(st.starts_with("zd_") && ft.starts_with("zd_") && st != ft);
    assert_eq!(call(&r, "tok", "/api/account/device_create", json!({"name": "x", "scope": "root"})).status, 400);
    // the list never shows a token or a hash
    let list = call(&r, "tok", "/api/account/devices_list", json!({}));
    let shown = list.body.to_string();
    assert!(!shown.contains(&st) && !shown.contains(&devices::hash_token(&st)) && list.body["devices"].as_array().unwrap().len() == 2, "{shown}");
    // sync token: only /api/sync/*
    assert_eq!(call(&r, &st, "/api/sync/manifest", json!({})).status, 200);
    assert_eq!(call(&r, &st, "/api/op", json!({"op": "chats"})).status, 403);
    assert_eq!(call(&r, &st, "/api/send", json!({"text": "hi"})).status, 403);
    assert_eq!(call(&r, &st, "/api/reply", json!({"stream": 1})).status, 403);
    assert_eq!(call(&r, &st, "/api/account/whoami", json!({})).status, 403);
    // full token: the API and the sync, not the account
    assert_eq!(call(&r, &ft, "/api/op", json!({"op": "chats"})).status, 200);
    assert_eq!(call(&r, &ft, "/api/sync/manifest", json!({})).status, 200);
    assert_eq!(call(&r, &ft, "/api/account/devices_list", json!({})).status, 403);
    assert_eq!(call(&r, &ft, "/api/account/delete", json!({"confirm": "DELETE"})).status, 403, "a device cannot delete the account");
    // a device sees the same data as its owner
    assert_eq!(call(&r, &ft, "/api/sync/put", json!({"kind": "chats", "id": "dc", "doc": chat("dc", "from the terminal")})).status, 200);
    assert!(call(&r, "tok", "/api/op", json!({"op": "chats"})).body.to_string().contains("from the terminal"));
    // made-up and malformed tokens
    assert_eq!(call(&r, &format!("zd_{}", "A".repeat(43)), "/api/sync/manifest", json!({})).status, 401);
    assert_eq!(call(&r, "zd_short", "/api/sync/manifest", json!({})).status, 401);
    assert_eq!(call(&r, "", "/api/sync/manifest", json!({})).status, 401);
    // revoking
    let id = list.body["devices"].as_array().unwrap().iter().find(|d| d["name"] == "terminal").unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(call(&r, "tok", "/api/account/device_revoke", json!({"id": id})).status, 200);
    assert_eq!(call(&r, &ft, "/api/op", json!({"op": "chats"})).status, 401, "revoked");
    assert_eq!(call(&r, &st, "/api/sync/manifest", json!({})).status, 200, "the other one still works");
    assert_eq!(call(&r, "tok", "/api/account/device_revoke", json!({"id": id})).status, 404);
}

#[test]
fn another_persons_device_cannot_be_revoked_and_a_token_belongs_to_its_own_tenant() {
    let (ua, ub) = (uniq("dv-a"), uniq("dv-b"));
    let r = rig(&[("tok-a", &ua, "a@x.test"), ("tok-b", &ub, "b@x.test")], "", 200, 600);
    let t = call(&r, "tok-a", "/api/account/device_create", json!({"name": "a", "scope": "full"}));
    let (token, id) = (t.body["token"].as_str().unwrap().to_string(), t.body["id"].as_str().unwrap().to_string());
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
    let t = call(&r, "tok-o", "/api/account/device_create", json!({"name": "o", "scope": "full"}));
    let token = t.body["token"].as_str().unwrap();
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
    let dev = call(&r, "tok-a", "/api/account/device_create", json!({"name": "d", "scope": "full"}));
    let dev_b = call(&r, "tok-b", "/api/account/device_create", json!({"name": "d", "scope": "sync"}));
    let (token_a, token_b) = (dev.body["token"].as_str().unwrap().to_string(), dev_b.body["token"].as_str().unwrap().to_string());
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
    let app = Arc::new(App { no_auth: true, noauth_admin: false, verifier: None, policy: Policy::default(), tenants, devices: None, streams: Mutex::new(HashMap::new()), next: AtomicU64::new(1), conns: AtomicUsize::new(0), legal: Default::default(), require_terms: std::sync::atomic::AtomicBool::new(false), accepted: Mutex::new(Default::default()) });
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
    // no devices without a database
    let d = post("/api/account/device_create", json!({"name": "x", "scope": "sync"}), true, None);
    assert_eq!(d.status(), 400);
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
    assert_eq!(call(&r, "tok-t", "/api/account/device_create", json!({"name": "x", "scope": "sync"})).status, 403, "no device token before accepting");
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
