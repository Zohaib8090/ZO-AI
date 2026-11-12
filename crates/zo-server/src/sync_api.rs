//! The calls the desktop app's sync makes: /api/sync/manifest, /get, /put and /delete. They use the same functions as the app
//! (zo-core's `sync`), on the server's own files, so the Firestore push picks every change up like any other.

use serde_json::{json, Value};

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Answers one /api/sync/* call. Err is shown to the caller as {"ok": false, "error": ...} with status 400.
pub fn handle(path: &str, v: &Value) -> Result<Value, String> {
    match path {
        "/api/sync/manifest" => {
            let mut m = zo_core::sync::manifest_json();
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
    fn put_get_manifest_delete_work_through_the_api() {
        let dir = std::env::temp_dir().join(format!("zo-syncapi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        let doc = json!({"id": "c1", "title": "t", "updated": 100, "entries": []});
        assert_eq!(handle("/api/sync/put", &json!({"kind": "chats", "id": "c1", "doc": doc})).unwrap()["applied"], true);
        assert_eq!(handle("/api/sync/put", &json!({"kind": "chats", "id": "c1", "doc": doc})).unwrap()["applied"], false);
        assert_eq!(handle("/api/sync/manifest", &json!({})).unwrap()["chats"]["c1"], 100);
        assert_eq!(handle("/api/sync/get", &json!({"kind": "chats", "id": "c1"})).unwrap()["doc"]["title"], "t");
        assert!(handle("/api/sync/get", &json!({"kind": "chats", "id": "nope"})).is_err());
        assert_eq!(handle("/api/sync/delete", &json!({"kind": "chats", "id": "c1", "at": zo_core::store::chats::now_millis()})).unwrap()["deleted"], true);
        assert!(handle("/api/sync/manifest", &json!({})).unwrap()["deleted"]["chats:c1"].is_u64());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
