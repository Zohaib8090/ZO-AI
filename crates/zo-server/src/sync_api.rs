//! The calls the desktop app's sync makes: /api/sync/manifest, /get, /put and /delete. They use the same functions as the app
//! (zo-core's `sync`), run as the caller's own tenant, so they only ever see that person's files, and the Firestore push picks every
//! change up like any other.

use serde_json::{json, Value};
use zo_core::tenant::{self, Tenant};

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Answers one /api/sync/* call for one person. Err is shown to the caller as {"ok": false, "error": ...} with status 400.
///
/// `keys_ok` is true only for the desktop app's own sync credential: provider API keys never travel to a web session (a Firebase
/// sign-in) or to a general device token, whatever the person's opt-in on the desktop (the server cannot see that opt-in, and a
/// key typed on the web must not be readable back from a stolen token).
pub fn handle(path: &str, v: &Value, who: &Tenant, keys_ok: bool) -> Result<Value, String> {
    tenant::scope(who, || run(path, v, keys_ok))
}

fn run(path: &str, v: &Value, keys_ok: bool) -> Result<Value, String> {
    if !keys_ok && text(v, "kind") == "keys" && path != "/api/sync/manifest" {
        return Err("API keys are not available over this sign-in. Use the desktop app's sync, or type the key again.".into());
    }
    match path {
        "/api/sync/manifest" => {
            let mut m = zo_core::sync::manifest_json();
            if !keys_ok {
                m["keys"] = json!({});
                if let Some(d) = m.get_mut("deleted").and_then(Value::as_object_mut) {
                    d.retain(|k, _| !k.starts_with("keys:"));
                }
            }
            m["ok"] = json!(true);
            Ok(m)
        }
        "/api/sync/get" => {
            let (kind, id) = (text(v, "kind"), text(v, "id"));
            match zo_core::sync::get_doc(kind, id) {
                Some(doc) => Ok(json!({"ok": true, "doc": doc})),
                None => Err("There is no such item.".into()),
            }
        }
        "/api/sync/put" => {
            let doc = v.get("doc").filter(|d| d.is_object()).ok_or("The request has no document.")?;
            tenant::check_room()?;
            let applied = zo_core::sync::put_doc(text(v, "kind"), text(v, "id"), doc)?;
            Ok(json!({"ok": true, "applied": applied}))
        }
        "/api/sync/delete" => {
            let at = v.get("at").and_then(Value::as_u64).ok_or("The request has no time.")?;
            let deleted = zo_core::sync::apply_delete(text(v, "kind"), text(v, "id"), at)?;
            Ok(json!({"ok": true, "deleted": deleted}))
        }
        _ => Err("Unknown sync call.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_manifest_delete_work_through_the_api_and_stay_in_the_callers_folders() {
        let root = crate::testenv::env();
        let a = Tenant::open_in(&root, &"a1".repeat(16), true).unwrap();
        let b = Tenant::open_in(&root, &"b2".repeat(16), true).unwrap();
        let doc = json!({"id": "c1", "title": "t", "updated": 100, "entries": []});
        assert_eq!(handle("/api/sync/put", &json!({"kind": "chats", "id": "c1", "doc": doc}), &a, true).unwrap()["applied"], true);
        assert_eq!(handle("/api/sync/put", &json!({"kind": "chats", "id": "c1", "doc": doc}), &a, true).unwrap()["applied"], false);
        assert_eq!(handle("/api/sync/manifest", &json!({}), &a, true).unwrap()["chats"]["c1"], 100);
        assert_eq!(handle("/api/sync/get", &json!({"kind": "chats", "id": "c1"}), &a, true).unwrap()["doc"]["title"], "t");
        // the other person sees nothing of it
        assert!(handle("/api/sync/get", &json!({"kind": "chats", "id": "c1"}), &b, true).is_err());
        assert!(handle("/api/sync/manifest", &json!({}), &b, true).unwrap()["chats"].get("c1").is_none());
        assert!(handle("/api/sync/get", &json!({"kind": "chats", "id": "nope"}), &a, true).is_err());
        assert_eq!(handle("/api/sync/delete", &json!({"kind": "chats", "id": "c1", "at": zo_core::store::chats::now_millis()}), &a, true).unwrap()["deleted"], true);
        assert!(handle("/api/sync/manifest", &json!({}), &a, true).unwrap()["deleted"]["chats:c1"].is_u64());
        assert!(handle("/api/sync/nothing", &json!({}), &a, true).is_err());
    }

    #[test]
    fn provider_keys_travel_only_to_the_desktops_own_sync_credential() {
        let root = crate::testenv::env();
        let a = Tenant::open_in(&root, &"c3".repeat(16), true).unwrap();
        tenant::scope(&a, || zo_core::ai::keys::set("custom", "sk-typed-on-the-web").unwrap());
        let get = json!({"kind": "keys", "id": "custom"});
        let desk = handle("/api/sync/get", &get, &a, true).unwrap();
        assert_eq!(desk["doc"]["key"], "sk-typed-on-the-web");
        for path in ["/api/sync/get", "/api/sync/put", "/api/sync/delete"] {
            let e = handle(path, &json!({"kind": "keys", "id": "custom", "doc": {"key": "x", "updated": 5}, "at": 5}), &a, false).unwrap_err();
            assert!(e.contains("not available"), "{path}: {e}");
        }
        let m = handle("/api/sync/manifest", &json!({}), &a, false).unwrap();
        assert!(m["keys"].as_object().unwrap().is_empty() && !m.to_string().contains("sk-typed"), "{m}");
        assert!(!handle("/api/sync/manifest", &json!({}), &a, true).unwrap()["keys"].as_object().unwrap().is_empty());
    }
}
