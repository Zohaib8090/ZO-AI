//! The terminal tools: terminals shown in the ZO-AI window that the AI opens, types into and reads. See `crate::term`.

use super::text;
use crate::term;
use serde_json::Value;
use std::time::Duration;

fn id_of(a: &Value) -> Result<u32, String> {
    a.get("id").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).map(|n| n as u32).ok_or_else(|| "missing argument: id (see zohara_term_list)".to_string())
}

fn find(a: &Value) -> Result<std::sync::Arc<term::Term>, String> {
    let id = id_of(a)?;
    term::get(id).ok_or_else(|| format!("There is no terminal {id}. See zohara_term_list."))
}

pub fn open(a: &Value) -> Result<String, String> {
    let name = a.get("name").and_then(Value::as_str).unwrap_or("");
    let cwd = a.get("cwd").and_then(Value::as_str).map(|d| super::expand(d).to_string_lossy().into_owned());
    let t = term::open(name, cwd.as_deref(), None, true)?;
    Ok(format!("Opened terminal {} (\"{}\"). It is shown in the ZO-AI window. Use zohara_term_run with id {} to run commands in it.", t.id, t.name(), t.id))
}

pub fn run(a: &Value) -> Result<String, String> {
    let cmd = text(a, "cmd")?;
    // no id: use the terminal the AI opened last, or open one
    let t = match a.get("id") {
        Some(_) => find(a)?,
        None => match term::list().into_iter().rev().find(|t| t.by_ai && t.alive()) {
            Some(t) => t,
            None => term::open("", None, None, true)?,
        },
    };
    if !t.alive() {
        return Err(format!("Terminal {} has ended.", t.id));
    }
    let secs = a.get("timeout").and_then(Value::as_u64).unwrap_or(30).clamp(1, 600);
    let out = t.run(cmd, Duration::from_secs(secs));
    Ok(format!("[terminal {}]\n{}", t.id, out))
}

pub fn send(a: &Value) -> Result<String, String> {
    let t = find(a)?;
    let mut sent = Vec::new();
    if let Some(s) = a.get("text").and_then(Value::as_str) {
        t.write(s.as_bytes());
        sent.push("text".to_string());
    }
    if let Some(k) = a.get("key").and_then(Value::as_str) {
        let bytes = term::key_bytes(k).ok_or_else(|| format!("unknown key \"{k}\" (try Enter, Tab, Escape, Up, Down, Left, Right, Backspace, Ctrl+C)"))?;
        t.write(&bytes);
        sent.push(format!("key {k}"));
    }
    if sent.is_empty() {
        return Err("give text or key".into());
    }
    std::thread::sleep(Duration::from_millis(600));
    Ok(format!("Sent {} to terminal {}.\n{}", sent.join(" and "), t.id, t.tail(40)))
}

pub fn read(a: &Value) -> Result<String, String> {
    let t = find(a)?;
    let n = a.get("lines").and_then(Value::as_u64).unwrap_or(60).clamp(1, 500) as usize;
    Ok(format!("[terminal {} \"{}\"{}]\n{}", t.id, t.name(), if t.alive() { "" } else { " (ended)" }, t.tail(n)))
}

pub fn list() -> Result<String, String> {
    let all = term::list();
    if all.is_empty() {
        return Ok("No terminals are open.".into());
    }
    Ok(all.iter().map(|t| format!("{}: \"{}\" ({}{})", t.id, t.name(), if t.by_ai { "opened by you, the AI" } else { "opened by the person" }, if t.alive() { "" } else { ", ended" })).collect::<Vec<_>>().join("\n"))
}

pub fn close(a: &Value) -> Result<String, String> {
    let t = find(a)?;
    t.close();
    Ok(format!("Closed terminal {}.", t.id))
}
