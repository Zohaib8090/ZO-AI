//! The agent service: ZO-AI's brain on a local socket, so a program that is not the window can chat with it, run its tools and
//! manage it. The first client is `zohara-linkd`, which carries it to the phone app (Zohara Link > ZoAI).
//!
//! Protocol: one JSON object per line. A client sends one request line; the answer is one line, or for `send` a stream of event
//! lines that ends with `{"event":"done"}`. While a `send` runs the client may send `{"op":"approve","id":N,"yes":true}` for an
//! approval the AI asked for, and `{"op":"stop"}` (closing the connection also stops it).
//!
//! The socket is private to the user (0600 inside a 0700 folder). Who may talk to it is decided by the client; `zohara-linkd` only
//! forwards for phones that were allowed ZO-AI on the computer.

use crate::ai::agent::{self, Config, Event};
use crate::ai::{claude_code, providers, Selection};
use crate::approval::Approver;
use crate::policy::Mode;
use crate::store::chats::{self, Chat, Entry};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/zo-ai-{}", unsafe { libc::getuid() })))
        .join("zo-ai");
    dir.join("agent.sock")
}

/// Starts listening (in the background). Safe to call once per process.
pub fn start() -> Result<(), String> {
    let path = socket_path();
    let dir = path.parent().ok_or("no folder for the socket")?.to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    // another ZO-AI is already serving: leave its socket alone
    if UnixStream::connect(&path).is_ok() {
        return Err("another ZO-AI already serves the phone socket".into());
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("cannot listen on {}: {e}", path.display()))?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            std::thread::spawn(move || serve(conn));
        }
    });
    Ok(())
}

fn line(w: &Mutex<UnixStream>, v: &Value) -> bool {
    let mut w = w.lock().unwrap();
    let mut s = v.to_string();
    s.push('\n');
    w.write_all(s.as_bytes()).is_ok()
}

fn serve(conn: UnixStream) {
    let Ok(read) = conn.try_clone() else { return };
    let writer = Arc::new(Mutex::new(conn));
    let mut reader = BufReader::new(read);
    let mut first = String::new();
    if reader.read_line(&mut first).unwrap_or(0) == 0 {
        return;
    }
    let req: Value = match serde_json::from_str(&first) {
        Ok(v) => v,
        Err(_) => {
            line(&writer, &json!({"ok": false, "error": "not JSON"}));
            return;
        }
    };
    let op = req.get("op").and_then(Value::as_str).unwrap_or("").to_string();
    if op == "send" {
        send(&req, reader, writer);
        return;
    }
    let reply = match handle(&op, &req) {
        Ok(mut v) => {
            if let Some(o) = v.as_object_mut() {
                o.insert("ok".into(), json!(true));
            }
            v
        }
        Err(e) => json!({"ok": false, "error": e}),
    };
    line(&writer, &reply);
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn handle(op: &str, req: &Value) -> Result<Value, String> {
    match op {
        "status" => {
            let sel = Selection::load();
            Ok(json!({
                "provider": sel.provider, "model": sel.model, "mode": Mode::load().name(),
                "ready": !sel.provider.is_empty(),
                "version": env!("CARGO_PKG_VERSION"),
            }))
        }
        "sync_status" => Ok(crate::sync::status_json()),
        "chats" => {
            let all = chats::load();
            Ok(json!({"chats": all.iter().take(60).map(|c| json!({"id": c.id, "title": c.title, "updated": c.updated, "pinned": c.pinned})).collect::<Vec<_>>()}))
        }
        "chat" => {
            let id = s(req, "id");
            let c = chats::load().into_iter().find(|c| c.id == id).ok_or("that chat is gone")?;
            Ok(json!({"id": c.id, "title": c.title, "entries": c.entries.iter().map(|e| json!({"kind": e.kind, "text": e.text})).collect::<Vec<_>>(),
                "takes": c.take_counts().iter().map(|(n, cur, count)| json!({"nth": n, "current": cur, "count": count})).collect::<Vec<_>>()}))
        }
        "switch" => {
            // show an other take of one of your messages
            let id = s(req, "id");
            let mut c = chats::load().into_iter().find(|c| c.id == id).ok_or("that chat is gone")?;
            let (nth, delta) = (req.get("nth").and_then(Value::as_u64).unwrap_or(0) as usize, req.get("delta").and_then(Value::as_i64).unwrap_or(0) as i32);
            if !c.switch_version(nth, delta) {
                return Err("There is no such take.".into());
            }
            c.branches = c.snapshot_branches();
            c.updated = chats::now_millis();
            chats::upsert(c);
            handle("chat", &json!({"id": id}))
        }
        "delete_chat" => {
            chats::delete(s(req, "id"));
            Ok(json!({}))
        }
        "pin_chat" => {
            chats::set_pinned(s(req, "id"), req.get("on").and_then(Value::as_bool).unwrap_or(true));
            Ok(json!({}))
        }
        "mode" => {
            if let Some(m) = req.get("set").and_then(Value::as_str) {
                Mode::parse(m).save();
            }
            Ok(json!({"mode": Mode::load().name()}))
        }
        "providers" => {
            let sel = Selection::load();
            Ok(json!({
                "current": sel.provider,
                "providers": providers::all().iter().map(|p| json!({
                    "id": p.id, "name": p.name, "local": p.local,
                    "has_key": p.local || p.protocol == crate::ai::providers::Protocol::ClaudeCode || crate::ai::profiles::get_key(&sel, p.id).is_some(),
                })).collect::<Vec<_>>(),
            }))
        }
        "models" => {
            let sel = Selection::load();
            let pid = req.get("provider").and_then(Value::as_str).unwrap_or(&sel.provider).to_string();
            let p = providers::find(&pid).ok_or("choose a provider on the computer first")?;
            let key = crate::ai::profiles::get_key(&sel, p.id);
            let list = agent::list_models(p, &sel.base_url_for(p), key.as_deref())?;
            Ok(json!({"provider": p.id, "models": list, "current": sel.model, "efforts": crate::ai::models::effort_levels(&sel.model), "effort": sel.effort}))
        }
        "select" => {
            let mut sel = Selection::load();
            if let Some(p) = req.get("provider").and_then(Value::as_str) {
                let prov = providers::find(p).ok_or("unknown provider")?;
                if sel.provider != p {
                    sel.provider = p.to_string();
                    // a new provider starts without the old provider's model
                    sel.model = prov.models.first().map(|m| m.to_string()).unwrap_or_default();
                    sel.effort = None;
                }
            }
            if let Some(m) = req.get("model").and_then(Value::as_str) {
                sel.model = m.to_string();
            }
            if let Some(e) = req.get("effort").and_then(Value::as_str) {
                sel.effort = Some(e.to_string()).filter(|e| !e.is_empty());
            }
            crate::ai::profiles::update_active(&sel);
            sel.save();
            Ok(json!({"provider": sel.provider, "model": sel.model, "effort": sel.effort}))
        }
        "set_key" => {
            let sel = Selection::load();
            let pid = req.get("provider").and_then(Value::as_str).unwrap_or(&sel.provider).to_string();
            providers::find(&pid).ok_or("unknown provider")?;
            let key = s(req, "key").trim().to_string();
            if key.is_empty() {
                return Err("The key is empty.".into());
            }
            crate::ai::keys::set(&crate::ai::profiles::key_id(&sel, &pid), &key)?;
            Ok(json!({}))
        }
        "profiles" => {
            let sel = Selection::load();
            Ok(json!({"active": sel.profile, "profiles": crate::ai::profiles::load().iter().map(|p| json!({
                "id": p.id, "name": p.name, "provider": p.provider, "model": p.model, "effort": p.effort, "pinned": p.pinned,
            })).collect::<Vec<_>>()}))
        }
        "profile_use" => {
            let id = s(req, "id");
            let p = crate::ai::profiles::load().into_iter().find(|p| p.id == id).ok_or("that profile is gone")?;
            let mut sel = Selection::load();
            sel.apply_profile(&p);
            sel.profile = Some(p.id.clone());
            sel.save();
            Ok(json!({"provider": sel.provider, "model": sel.model, "effort": sel.effort}))
        }
        "profile_create" => {
            let mut sel = Selection::load();
            let p = crate::ai::profiles::create(s(req, "name"), &sel)?;
            sel.profile = Some(p.id.clone());
            sel.save();
            Ok(json!({"id": p.id}))
        }
        "profile_rename" => {
            crate::ai::profiles::rename(s(req, "id"), s(req, "name"))?;
            Ok(json!({}))
        }
        "profile_delete" => {
            let id = s(req, "id").to_string();
            crate::ai::profiles::delete(&id);
            let mut sel = Selection::load();
            if sel.profile.as_deref() == Some(id.as_str()) {
                sel.profile = None;
                sel.save();
            }
            Ok(json!({}))
        }
        "profile_pin" => {
            let (id, model) = (s(req, "id"), s(req, "model"));
            if req.get("on").and_then(Value::as_bool).unwrap_or(true) {
                crate::ai::profiles::pin(id, model);
            } else {
                crate::ai::profiles::unpin(id, model);
            }
            Ok(json!({}))
        }
        "scheduled" => Ok(json!({"tasks": crate::store::schedules::load().iter().map(|t| json!({
            "id": t.id, "title": t.title, "prompt": t.prompt, "next_run": t.next_run, "repeat": t.repeat, "enabled": t.enabled,
        })).collect::<Vec<_>>()})),
        "schedule_cancel" => {
            if crate::store::schedules::remove(s(req, "id")) { Ok(json!({})) } else { Err("That task is gone.".into()) }
        }
        "schedule_add" => {
            let mut a = json!({"task": s(req, "task"), "when": s(req, "when")});
            for k in ["repeat", "title"] {
                if let Some(v) = req.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    a[k] = json!(v);
                }
            }
            Ok(json!({"message": crate::tools::call("zohara_ai_schedule", &a)?}))
        }
        "machines" => Ok(json!({"machines": crate::store::machines::load().iter().map(|m| json!({
            "name": m.name, "host": m.host, "user": m.user, "port": m.port, "note": m.note, "has_key": m.has_key(),
        })).collect::<Vec<_>>()})),
        "machine_add" => {
            let m = crate::store::machines::Machine {
                name: s(req, "name").trim().into(),
                host: s(req, "host").trim().into(),
                user: s(req, "user").trim().into(),
                port: match req.get("port") {
                    Some(Value::Number(n)) => n.as_u64().filter(|p| (1..=65535).contains(p)).ok_or("The port must be a number from 1 to 65535.")? as u16,
                    Some(Value::String(t)) => crate::store::machines::parse_port(t)?,
                    _ => 22,
                },
                note: s(req, "note").trim().into(),
            };
            crate::store::machines::add(m, req.get("key").and_then(Value::as_str))?;
            Ok(json!({}))
        }
        "machine_remove" => crate::store::machines::remove(s(req, "name")).map(|_| json!({})),
        "machine_test" => crate::tools::ssh::test(s(req, "name")).map(|m| json!({"message": m})),
        "projects" => Ok(json!({"text": crate::tools::call("zohara_ai_projects", &json!({}))?})),
        "project_create" => Ok(json!({"message": crate::tools::call("zohara_ai_create_project", &json!({"name": s(req, "name"), "goal": s(req, "goal")}))?})),
        "compact" => {
            // summarize the older part of a chat, as the Compact session button does on the computer
            let id = s(req, "id");
            let mut c = chats::load().into_iter().find(|c| c.id == id).ok_or("that chat is gone")?;
            let sel = Selection::load();
            if sel.provider == "claude-code" {
                return Err("Claude Code keeps and compacts this conversation itself.".into());
            }
            let cfg = Config::from_selection(&sel, agent::SYSTEM_PROMPT)?;
            agent::compact(&cfg, &mut c.history, &AtomicBool::new(false), &mut |_| {})?;
            c.entries.push(entry("msg-tool", "The earlier part of this conversation was summarized, so there is more room to go on."));
            chats::upsert(c);
            Ok(json!({}))
        }
        "memory" => {
            // the long-term memory, for the phone's Memory page
            let st = crate::store::memory::load();
            let mut items = st.items.clone();
            items.sort_by(|a, b| (b.pinned, b.updated).cmp(&(a.pinned, a.updated)));
            Ok(json!({"enabled": st.enabled, "every": st.every, "items": items.iter().take(300).map(|i| json!({"id": i.id, "text": i.text, "kind": i.kind, "title": i.title, "chat": i.chat, "pinned": i.pinned, "updated": i.updated})).collect::<Vec<_>>()}))
        }
        "memory_add" => {
            let (_, new) = crate::store::memory::remember(s(req, "text"), if s(req, "kind").is_empty() { "fact" } else { s(req, "kind") }, "").ok_or("That note is too short.")?;
            Ok(json!({"new": new}))
        }
        "memory_update" => {
            if crate::store::memory::update_text(s(req, "id"), s(req, "text")) { Ok(json!({})) } else { Err("That note is gone or the text is too short.".into()) }
        }
        "memory_edit" => Ok(json!({"message": crate::ai::memory_agent::edit_by_instruction(s(req, "text"))?})),
        "search" => {
            let all = chats::load();
            Ok(json!({"hits": chats::search(s(req, "query"), &all).iter().take(50).map(|h| json!({"id": h.id, "title": h.title, "snippet": h.snippet, "updated": h.updated})).collect::<Vec<_>>()}))
        }
        "artifacts" => Ok(json!({"items": crate::store::artifacts::load().iter().take(100).map(|a| json!({
            "id": a.id, "kind": a.kind, "title": a.title, "path": a.path, "text": a.text.as_ref().map(|t| t.chars().take(20000).collect::<String>()), "chat": a.chat, "created": a.created,
        })).collect::<Vec<_>>()})),
        "memory_forget" => {
            crate::store::memory::forget(s(req, "id"));
            Ok(json!({}))
        }
        "memory_pin" => {
            crate::store::memory::set_pinned(s(req, "id"), req.get("on").and_then(Value::as_bool).unwrap_or(true));
            Ok(json!({}))
        }
        "memory_enable" => {
            crate::store::memory::set_enabled(req.get("on").and_then(Value::as_bool).unwrap_or(true));
            Ok(json!({}))
        }
        "memory_clear" => {
            crate::store::memory::clear_all();
            Ok(json!({}))
        }
        "personalities" => {
            let st = crate::store::personality::load();
            let builtin = crate::store::personality::builtin();
            Ok(json!({"active": st.active, "items": crate::store::personality::all().iter().map(|p| json!({"id": p.id, "name": p.name, "prompt": p.prompt, "generated": p.generated, "builtin": builtin.iter().any(|b| b.id == p.id)})).collect::<Vec<_>>()}))
        }
        "personality_use" => {
            crate::store::personality::set_active(s(req, "id"))?;
            Ok(json!({}))
        }
        "personality_add" => {
            let p = crate::store::personality::add(s(req, "name"), s(req, "prompt"), false)?;
            Ok(json!({"id": p.id}))
        }
        "personality_generate" => {
            let p = crate::ai::personality_gen::generate(s(req, "description"))?;
            Ok(json!({"id": p.id, "name": p.name, "prompt": p.prompt}))
        }
        "personality_remove" => {
            crate::store::personality::remove(s(req, "id"));
            Ok(json!({}))
        }
        "mcp_list" => {
            let all = crate::mcp_client::load_servers();
            Ok(json!({"servers": all.iter().map(|s| json!({"name": s.name, "command": s.command, "enabled": s.enabled})).collect::<Vec<_>>()}))
        }
        "mcp_add" => {
            let mut a = json!({"name": s(req, "name"), "address": s(req, "address")});
            if let Some(t) = req.get("token").and_then(Value::as_str) {
                a["token"] = json!(t);
            }
            Ok(json!({"message": crate::tools::call("zohara_ai_add_mcp_server", &a)?}))
        }
        "mcp_import" => Ok(json!({"message": crate::tools::call("zohara_ai_add_mcp_config", &json!({"config": s(req, "config")}))?})),
        "mcp_remove" => {
            let name = s(req, "name");
            let mut all = crate::mcp_client::load_servers();
            let before = all.len();
            all.retain(|x| x.name != name);
            if all.len() == before {
                return Err(format!("no server named {name}"));
            }
            crate::mcp_client::save_servers(&all);
            Ok(json!({}))
        }
        "mcp_toggle" => {
            let (name, on) = (s(req, "name"), req.get("enabled").and_then(Value::as_bool).unwrap_or(true));
            let mut all = crate::mcp_client::load_servers();
            let x = all.iter_mut().find(|x| x.name == name).ok_or("no such server")?;
            x.enabled = on;
            crate::mcp_client::save_servers(&all);
            Ok(json!({}))
        }
        other => Err(format!("unknown request: {other}")),
    }
}

/// Asks the connected client (the phone) for a yes.
struct RemoteApprover {
    out: Arc<Mutex<UnixStream>>,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<bool>>>>,
    next: AtomicU64,
}

impl Approver for RemoteApprover {
    fn approve(&self, tool: &str, summary: &str) -> bool {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if !line(&self.out, &json!({"event": "approval", "id": id, "tool": tool, "summary": summary})) {
            return false;
        }
        let yes = rx.recv_timeout(Duration::from_secs(180)).unwrap_or(false);
        self.pending.lock().unwrap().remove(&id);
        yes
    }
}

fn entry(kind: &str, text: impl Into<String>) -> Entry {
    Entry { kind: kind.into(), text: text.into() }
}

fn send(req: &Value, mut reader: BufReader<UnixStream>, out: Arc<Mutex<UnixStream>>) {
    let mut text = s(req, "text").trim().to_string();
    let edit = req.get("edit").and_then(Value::as_u64).map(|n| n as usize);
    if text.is_empty() && edit.is_none() {
        line(&out, &json!({"event": "error", "error": "empty message"}));
        line(&out, &json!({"event": "done"}));
        return;
    }
    let sel = Selection::load();
    if sel.provider.is_empty() {
        line(&out, &json!({"event": "error", "error": "No model is chosen on the computer yet. Open ZO-AI there and choose one."}));
        line(&out, &json!({"event": "done"}));
        return;
    }
    // continue a saved chat, or start one
    let mut chat = match req.get("chat").and_then(Value::as_str).filter(|c| !c.is_empty()) {
        Some(id) => chats::load().into_iter().find(|c| c.id == id).unwrap_or_else(|| new_chat(&text)),
        None => new_chat(&text),
    };
    // editing or asking again: what is there from that message on is kept as an earlier take
    if let Some(n) = edit {
        match chat.fork_at(n) {
            Some(old) => {
                if text.is_empty() {
                    text = old;
                }
            }
            None => {
                line(&out, &json!({"event": "error", "error": "That message is gone."}));
                line(&out, &json!({"event": "done"}));
                return;
            }
        }
    }
    line(&out, &json!({"event": "chat", "id": chat.id, "title": chat.title}));
    if let Some(m) = req.get("mode").and_then(Value::as_str) {
        Mode::parse(m).save();
    }
    let mode = Mode::load();

    // the phone's answers and "stop" arrive while the AI works
    let cancel = Arc::new(AtomicBool::new(false));
    let pending: Arc<Mutex<HashMap<u64, mpsc::Sender<bool>>>> = Arc::new(Mutex::new(HashMap::new()));
    {
        let (cancel, pending) = (cancel.clone(), pending.clone());
        std::thread::spawn(move || {
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) | Err(_) => {
                        cancel.store(true, Ordering::SeqCst);
                        // nobody is left to answer
                        pending.lock().unwrap().clear();
                        return;
                    }
                    Ok(_) => {
                        let Ok(v) = serde_json::from_str::<Value>(&buf) else { continue };
                        match v.get("op").and_then(Value::as_str) {
                            Some("approve") => {
                                let id = v.get("id").and_then(Value::as_u64).unwrap_or(u64::MAX);
                                if let Some(tx) = pending.lock().unwrap().get(&id) {
                                    let _ = tx.send(v.get("yes").and_then(Value::as_bool).unwrap_or(false));
                                }
                            }
                            Some("stop") => cancel.store(true, Ordering::SeqCst),
                            _ => {}
                        }
                    }
                }
            }
        });
    }
    let approver = RemoteApprover { out: out.clone(), pending, next: AtomicU64::new(1) };

    chat.entries.push(entry("msg-user", text.clone()));
    let system = format!("{}\n\nThe person is talking to you from their phone (Zohara Link). Keep answers short and easy to read on a small screen.", crate::store::memory::system_with(agent::SYSTEM_PROMPT, &text));
    let mut session = chat.session.clone();
    let mut history = chat.history.clone();
    let mut entries = std::mem::take(&mut chat.entries);
    let result = {
        let o = out.clone();
        let mut on = |e: Event| match e {
            Event::TextDelta(d) => {
                line(&o, &json!({"event": "delta", "text": d}));
            }
            Event::Text(t) => {
                entries.push(entry("msg-ai", t.clone()));
                line(&o, &json!({"event": "text", "text": t}));
            }
            Event::ToolStart { name, summary } => {
                entries.push(entry("msg-tool", format!("▸ {name}  {}", summary.lines().nth(1).unwrap_or(""))));
                line(&o, &json!({"event": "tool_start", "name": name, "summary": summary}));
            }
            Event::ToolDone { name, ok, output } => {
                let short: String = output.chars().take(300).collect();
                entries.push(entry(if ok { "msg-tool-out" } else { "msg-error" }, format!("{} {short}", if ok { "✓" } else { "✗" })));
                // the phone gets a short version: tool output can be long
                let cut: String = output.chars().take(1500).collect();
                line(&o, &json!({"event": "tool_done", "name": name, "ok": ok, "output": cut}));
            }
            Event::Usage(u) => {
                line(&o, &json!({"event": "usage", "input": u.input, "output": u.output}));
            }
        };
        if sel.provider == "claude-code" {
            let model = if sel.model.is_empty() { "sonnet".to_string() } else { sel.model.clone() };
            claude_code::run(&model, sel.effort.as_deref(), &mut session, &text, &system, &cancel, &mut on)
        } else {
            match Config::from_selection(&sel, &system) {
                Ok(cfg) => agent::run(&cfg, &mut history, &text, mode, &approver, &cancel, &mut on),
                Err(e) => Err(e),
            }
        }
    };
    if let Err(e) = result {
        if e != agent::STOPPED {
            entries.push(entry("msg-error", e.clone()));
            line(&out, &json!({"event": "error", "error": e}));
        } else {
            line(&out, &json!({"event": "stopped"}));
        }
    }
    chat.entries = entries;
    chat.history = history;
    chat.session = session;
    chat.branches = chat.snapshot_branches();
    chat.updated = chats::now_millis();
    crate::ai::memory_agent::after_turn(&chat.id, &chat.title, &chat.entries);
    chats::upsert(chat);
    line(&out, &json!({"event": "done"}));
    // the helper thread that listens for answers still holds the other end: close the connection so the client sees the end
    let _ = out.lock().unwrap().shutdown(std::net::Shutdown::Both);
}

fn new_chat(first: &str) -> Chat {
    let now = chats::now_millis();
    let title: String = first.chars().take(40).collect();
    Chat { id: format!("c{now}"), title, updated: now, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A tiny model server that answers one request.
    fn model(answer: Value) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = s.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                    let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                    while buf.len() < i + 4 + len {
                        let n = s.read(&mut tmp).unwrap();
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    break;
                }
            }
            let body = answer.to_string();
            write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        url
    }

    #[test]
    fn a_message_comes_back_as_events_and_is_saved_as_a_chat() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-service-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        let url = model(json!({"choices":[{"message":{"content":"Hello from the laptop."}}]}));
        let mut sel = Selection { provider: "ollama".into(), model: "m".into(), ..Default::default() };
        sel.base_urls.insert("ollama".into(), url);
        sel.save();

        let (client, server) = UnixStream::pair().unwrap();
        std::thread::spawn(move || serve(server));
        let mut w = client.try_clone().unwrap();
        writeln!(w, "{}", json!({"op": "send", "text": "hi from the phone"})).unwrap();
        let mut events = Vec::new();
        for l in BufReader::new(client).lines() {
            let v: Value = serde_json::from_str(&l.unwrap()).unwrap();
            let done = v["event"] == "done";
            events.push(v);
            if done {
                break;
            }
        }
        let kinds: Vec<String> = events.iter().map(|e| e["event"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds.first().map(String::as_str), Some("chat"), "{kinds:?}");
        assert!(events.iter().any(|e| e["event"] == "text" && e["text"] == "Hello from the laptop."), "{events:?}");
        let saved = chats::load();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].title, "hi from the phone");
        assert!(saved[0].entries.iter().any(|e| e.kind == "msg-ai"));

        // the other requests
        let r = handle("chats", &json!({})).unwrap();
        assert_eq!(r["chats"].as_array().unwrap().len(), 1);
        assert_eq!(handle("mode", &json!({"set": "manual"})).unwrap()["mode"], "Manual");
        assert!(handle("nonsense", &json!({})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn editing_a_message_over_the_socket_keeps_the_old_take() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-service-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        let e = |k: &str, t: &str| chats::Entry { kind: k.into(), text: t.into() };
        chats::upsert(Chat {
            id: "c1".into(), title: "t".into(),
            entries: vec![e("msg-user", "one"), e("msg-ai", "a1")],
            history: vec![crate::ai::wire::Turn::User("one".into()), crate::ai::wire::Turn::Assistant { text: "a1".into(), calls: vec![] }],
            ..Default::default()
        });
        // the part that does not need a model: fork, switch, and the counts
        let mut c = chats::load().remove(0);
        assert_eq!(c.fork_at(0).as_deref(), Some("one"));
        c.entries.push(e("msg-user", "one, edited"));
        c.branches = c.snapshot_branches();
        chats::upsert(c);
        let r = handle("chat", &json!({"id": "c1"})).unwrap();
        assert_eq!(r["takes"][0]["count"], 2);
        let r = handle("switch", &json!({"id": "c1", "nth": 0, "delta": -1})).unwrap();
        assert_eq!(r["entries"][0]["text"], "one");
        assert_eq!(r["entries"].as_array().unwrap().len(), 2);
        assert!(handle("switch", &json!({"id": "c1", "nth": 0, "delta": -1})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
