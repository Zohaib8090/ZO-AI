//! Claude Code mode: ZO-AI runs the real `claude` program that the person installed and signed in to themselves,
//! and connects it to `zo-mcp`, so Claude does the thinking with its own login and ZO-AI only supplies the
//! computer controls and the approvals. ZO-AI never reads, copies or stores any Claude credential.

use super::agent::{Event, SYSTEM_PROMPT};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Where `claude` is: `ZOAI_CLAUDE_BIN`, the PATH, or `~/.local/bin`.
pub fn binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ZOAI_CLAUDE_BIN") {
        return Some(PathBuf::from(p));
    }
    let on_path = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("claude")).find(|p| p.is_file()));
    on_path.or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/claude")).filter(|p| p.is_file()))
}

/// `zo-mcp` next to the running program, else whatever the PATH finds.
fn mcp_binary() -> String {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("zo-mcp"))).filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "zo-mcp".into())
}

fn tool_name(raw: &str) -> &str {
    raw.strip_prefix("mcp__zohara__").unwrap_or(raw)
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

/// Turn one line of `--output-format stream-json` into events. Returns `Some(error)` when the run itself failed.
fn handle_line(v: &Value, session: &mut Option<String>, names: &mut Vec<(String, String)>, on_event: &mut dyn FnMut(Event)) -> Option<String> {
    if let Some(id) = v["session_id"].as_str() {
        *session = Some(id.to_string());
    }
    match v["type"].as_str() {
        // the text as Claude writes it
        Some("stream_event") => {
            if let Some(t) = v["event"]["delta"]["text"].as_str().filter(|t| !t.is_empty()) {
                on_event(Event::TextDelta(t.to_string()));
            }
            None
        }
        Some("assistant") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("text") => {
                        let t = b["text"].as_str().unwrap_or("");
                        if !t.is_empty() {
                            on_event(Event::Text(t.to_string()));
                        }
                    }
                    Some("tool_use") => {
                        let name = tool_name(b["name"].as_str().unwrap_or("")).to_string();
                        names.push((b["id"].as_str().unwrap_or("").to_string(), name.clone()));
                        on_event(Event::ToolStart { name: name.clone(), summary: crate::mcp::summarize(&name, &b["input"]) });
                    }
                    _ => {}
                }
            }
            None
        }
        Some("user") => {
            for b in v["message"]["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "tool_result") {
                let id = b["tool_use_id"].as_str().unwrap_or("");
                let name = names.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()).unwrap_or_default();
                on_event(Event::ToolDone { name, ok: b["is_error"] != true, output: result_text(&b["content"]) });
            }
            None
        }
        Some("result") if v["is_error"] == true => Some(v["result"].as_str().unwrap_or("Claude Code reported an error.").to_string()),
        _ => None,
    }
}

/// Send one message. `session` is remembered between calls so the conversation continues.
pub fn run(model: &str, effort: Option<&str>, session: &mut Option<String>, prompt: &str, system_extra: &str, cancel: &Arc<AtomicBool>, on_event: &mut dyn FnMut(Event)) -> Result<(), String> {
    let system = if system_extra.trim().is_empty() { SYSTEM_PROMPT.to_string() } else { format!("{SYSTEM_PROMPT}\n\n{system_extra}") };
    // Claude can look at what the tools return as pictures (a screenshot comes back as an image)
    let system = format!("{system}\n\n{}", super::agent::VISION_YES);
    let bin = binary().ok_or("Claude Code is not installed. Install it (npm install -g @anthropic-ai/claude-code), then sign in with: claude auth login")?;
    let config = json!({ "mcpServers": { "zohara": { "command": mcp_binary(), "env": { "ZO_MCP_EXTERNAL": "1" } } } }).to_string();
    let mut cmd = Command::new(bin);
    cmd.args(["-p", prompt, "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--strict-mcp-config", "--mcp-config", &config,
        // Claude's own file and shell tools are off: everything goes through ZO-AI's tools, so ZO-AI's permission modes apply
        "--tools", "", "--allowedTools", "mcp__zohara__*", "--append-system-prompt", &system]);
    if !model.is_empty() {
        cmd.args(["--model", model]);
    }
    // how hard it thinks: only the levels the program knows
    if let Some(level) = effort.filter(|l| ["low", "medium", "high", "xhigh", "max"].contains(l)) {
        cmd.args(["--effort", level]);
    }
    if let Some(id) = session.as_deref() {
        cmd.args(["--resume", id]);
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Could not start Claude Code: {e}"))?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let stderr_pipe = child.stderr.take();
    let child = Arc::new(Mutex::new(child));
    // pressing Stop ends the program
    let finished = Arc::new(AtomicBool::new(false));
    {
        let (child, cancel, finished) = (child.clone(), cancel.clone(), finished.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    if let Ok(mut c) = child.lock() {
                        let _ = c.kill();
                    }
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    }
    let mut failure = None;
    let mut names = Vec::new();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(e) = handle_line(&v, session, &mut names, on_event) {
                failure = Some(e);
            }
        }
    }
    let mut stderr = String::new();
    if let Some(mut e) = stderr_pipe {
        let _ = e.read_to_string(&mut stderr);
    }
    let status = child.lock().map_err(|_| "lock".to_string())?.wait().map_err(|e| e.to_string())?;
    finished.store(true, Ordering::Relaxed);
    if cancel.load(Ordering::Relaxed) {
        return Err(super::agent::STOPPED.into());
    }
    match (failure, status.success()) {
        (Some(e), _) => Err(e),
        (None, true) => Ok(()),
        (None, false) => {
            let why: String = stderr.trim().chars().take(300).collect();
            Err(format!("Claude Code stopped with an error{}. If you are not signed in, run: claude auth login", if why.is_empty() { String::new() } else { format!(": {why}") }))
        }
    }
}

/// One plain question to Claude Code, no tools and nothing saved: the answer as text. Used for small background jobs such as the
/// memory agent.
pub fn complete(model: &str, system: &str, prompt: &str, cancel: &Arc<AtomicBool>) -> Result<String, String> {
    let bin = binary().ok_or("Claude Code is not installed.")?;
    let mut cmd = Command::new(bin);
    cmd.args(["-p", prompt, "--output-format", "stream-json", "--verbose", "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}", "--tools", "", "--no-session-persistence", "--system-prompt", system]);
    if !model.is_empty() {
        cmd.args(["--model", model]);
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Could not start Claude Code: {e}"))?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let child = Arc::new(Mutex::new(child));
    let finished = Arc::new(AtomicBool::new(false));
    {
        let (child, cancel, finished) = (child.clone(), cancel.clone(), finished.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    if let Ok(mut c) = child.lock() {
                        let _ = c.kill();
                    }
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    }
    let (mut text, mut failure, mut session, mut names) = (String::new(), None, None, Vec::new());
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(e) = handle_line(&v, &mut session, &mut names, &mut |ev| if let Event::Text(t) = ev { text.push_str(&t) }) {
                failure = Some(e);
            }
        }
    }
    let ok = child.lock().map_err(|_| "lock".to_string())?.wait().map(|s| s.success()).unwrap_or(false);
    finished.store(true, Ordering::Relaxed);
    match failure {
        Some(e) => Err(e),
        None if ok && !text.trim().is_empty() => Ok(text),
        None => Err("Claude Code gave no answer.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_becomes_text_and_tool_events_and_keeps_the_session() {
        let lines = [
            json!({"type":"system","subtype":"init","session_id":"s-1"}),
            json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"mcp__zohara__zohara_system","input":{}}]},"session_id":"s-1"}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"OS: Linux"}]}]}}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"Kernel: Linux"}]}}),
            json!({"type":"result","is_error":false,"result":"Kernel: Linux"}),
        ];
        let (mut session, mut names, mut out) = (None, Vec::new(), Vec::new());
        for l in &lines {
            assert!(handle_line(l, &mut session, &mut names, &mut |e| out.push(match e {
                Event::TextDelta(t) => format!("delta:{t}"),
                Event::Text(t) => format!("text:{t}"),
                Event::ToolStart { name, .. } => format!("start:{name}"),
                Event::ToolDone { name, ok, output } => format!("done:{name}:{ok}:{output}"),
                Event::Usage(_) => "usage".to_string(),
            })).is_none());
        }
        assert_eq!(out, ["start:zohara_system", "done:zohara_system:true:OS: Linux", "text:Kernel: Linux"]);
        assert_eq!(session.as_deref(), Some("s-1"));
    }

    #[test]
    fn partial_messages_become_text_deltas() {
        let mut out = Vec::new();
        let line = json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}});
        assert!(handle_line(&line, &mut None, &mut Vec::new(), &mut |e| out.push(match e { Event::TextDelta(t) => t, _ => "other".into() })).is_none());
        assert_eq!(out, ["Hel"]);
    }

    #[test]
    fn an_error_result_is_reported() {
        let e = handle_line(&json!({"type":"result","is_error":true,"result":"Not logged in"}), &mut None, &mut Vec::new(), &mut |_| {});
        assert_eq!(e.as_deref(), Some("Not logged in"));
    }

    /// A stand-in `claude` that prints a stream and records how it was called.
    #[test]
    fn run_drives_the_program_and_resumes_the_session() {
        let dir = std::env::temp_dir().join(format!("zo-fake-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("claude");
        let args_log = dir.join("args");
        std::fs::write(&script, format!("#!/bin/sh\n(echo \"$@\" | tr '\\n' ' '; echo) >> {}\n\
            echo '{{\"type\":\"system\",\"session_id\":\"fake-1\"}}'\n\
            echo '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"hi\"}}]}}}}'\n", args_log.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("ZOAI_CLAUDE_BIN", &script);

        let (mut session, mut texts) = (None, Vec::new());
        run("sonnet", None, &mut session, "one", "", &Arc::new(AtomicBool::new(false)), &mut |e| if let Event::Text(t) = e { texts.push(t) }).unwrap();
        run("sonnet", Some("high"), &mut session, "two", "", &Arc::new(AtomicBool::new(false)), &mut |_| {}).unwrap();
        assert_eq!(texts, ["hi"]);
        assert_eq!(session.as_deref(), Some("fake-1"));
        let log = std::fs::read_to_string(&args_log).unwrap();
        let calls: Vec<&str> = log.lines().collect();
        assert!(!calls[0].contains("--resume") && calls[1].contains("--resume fake-1"));
        assert!(!calls[0].contains("--effort") && calls[1].contains("--effort high"));
        assert!(calls[0].contains("--tools") && calls[0].contains("--allowedTools mcp__zohara__*") && calls[0].contains("--model sonnet"));
        std::env::remove_var("ZOAI_CLAUDE_BIN");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
