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
    // Claude Code is a program on the server: only for a person the operator allows (see `allowed_here`)
    if crate::tenant::is_restricted() && !allowed_here() {
        return None;
    }
    if let Some(p) = std::env::var_os("ZOAI_CLAUDE_BIN") {
        return Some(PathBuf::from(p));
    }
    let on_path = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("claude")).find(|p| p.is_file()));
    on_path.or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/claude")).filter(|p| p.is_file()))
}

/// The operator's switch `ZO_CLAUDE_CODE_USERS`: `admins` (the default; anything else than `all` means this) or `all` (every signed-in person
/// may sign in to Claude Code with their own subscription and use it on the server, with ZO-AI's few safe tools only).
pub fn users_all() -> bool {
    #[cfg(test)]
    if let Some(v) = TEST_USERS.with(|c| c.get()) {
        return v;
    }
    std::env::var("ZO_CLAUDE_CODE_USERS").map(|v| v.trim().eq_ignore_ascii_case("all")).unwrap_or(false)
}

#[cfg(test)]
thread_local! {
    static TEST_USERS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Tests only: this thread behaves as if `ZO_CLAUDE_CODE_USERS` were `all` (true) or `admins` (false).
#[cfg(test)]
pub(crate) fn test_users_all(v: Option<bool>) {
    TEST_USERS.with(|c| c.set(v));
}

#[cfg(test)]
pub(crate) fn test_users_get() -> Option<bool> {
    TEST_USERS.with(|c| c.get())
}

/// On a server: may the current tenant sign in to and use Claude Code here? Admins always; everybody else only when the operator
/// chose `all`. A request without a tenant never may. On a computer there are no tenants and this is false (it is about servers).
pub fn allowed_here() -> bool {
    crate::tenant::multi_tenant() && crate::tenant::current().is_some_and(|t| !t.restricted || users_all())
}

#[cfg(test)]
pub(crate) fn binary_path_for_manual_test() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("claude")).find(|p| p.is_file()))
}

/// The id of the Claude sign-in token in a tenant's encrypted key store (`keys.enc`), and of the time it was stored.
pub const TOKEN_KEY: &str = "claude-code-token";
const TOKEN_AT_KEY: &str = "claude-code-token-at";

/// What the person is told when the server has no Claude sign-in for them.
pub const NO_TOKEN: &str = "Claude Code is not set up on the server for your account yet. Log in to Claude Code: press Log in under Settings > Model and profiles > Claude Code (or in the model list).";
/// What the person is told when Claude turns the token down (expired, revoked, or not a token).
pub const TOKEN_REJECTED: &str = "Claude rejected the sign-in (it expired or was revoked): press Log in again under Settings > Model and profiles > Claude Code.";

/// The server's own token from `CLAUDE_CODE_OAUTH_TOKEN`, read at start and taken out of the environment (see `zo-server`'s
/// hardening). It is only a fallback for an admin who has not sent a token of their own.
static GLOBAL_TOKEN: Mutex<Option<String>> = Mutex::new(None);

pub fn set_global_token(token: Option<String>) {
    *GLOBAL_TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = token.filter(|t| !t.is_empty());
}

/// Looks like a token from `claude setup-token` (`sk-ant-oat01-...`): printable URL-safe characters only, a sensible length. This is
/// a check against pasted garbage, not a proof that Claude accepts it.
pub fn token_shape_ok(t: &str) -> bool {
    (30..=400).contains(&t.len()) && t.starts_with("sk-ant-") && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Stores the token for the current tenant (encrypted, in its key store). Never for a restricted tenant. Returns the time stored.
pub fn store_token(token: &str) -> Result<u64, String> {
    if !allowed_here() {
        return Err("Claude Code is not available to this account.".into());
    }
    let token = token.trim();
    if !token_shape_ok(token) {
        return Err("That does not look like a Claude token (it starts with sk-ant- and comes from `claude setup-token`).".into());
    }
    let at = now_secs();
    super::keys::set(TOKEN_KEY, token)?;
    super::keys::set(TOKEN_AT_KEY, &at.to_string())?;
    Ok(at)
}

/// Removes the current tenant's token.
pub fn clear_token() -> Result<(), String> {
    super::keys::clear(TOKEN_KEY);
    super::keys::clear(TOKEN_AT_KEY);
    if super::keys::get(TOKEN_KEY).is_some() {
        return Err("The token could not be removed. Try again.".into());
    }
    Ok(())
}

/// When the current tenant's own token was stored (None: they have none).
pub fn token_set_at() -> Option<u64> {
    super::keys::get(TOKEN_KEY)?;
    Some(super::keys::get(TOKEN_AT_KEY).and_then(|s| s.parse().ok()).unwrap_or(0))
}

/// The token a Claude Code run on the server uses for the current tenant: their own, else (admin only, since a restricted tenant
/// never gets this far) the server's fallback.
fn server_token() -> Option<String> {
    // the operator's own fallback token is for admins only: an ordinary person must never run on the owner's subscription
    let admin = crate::tenant::current().is_some_and(|t| !t.restricted);
    super::keys::get(TOKEN_KEY).or_else(|| if admin { GLOBAL_TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone() } else { None })
}

/// Where the current tenant's Claude sign-in comes from on a server: `own` (they sent it), `server` (the operator's fallback, for an
/// admin) or `none`.
pub fn token_source() -> &'static str {
    if !allowed_here() {
        "none"
    } else if super::keys::get(TOKEN_KEY).is_some() {
        "own"
    } else if crate::tenant::current().is_some_and(|t| !t.restricted) && GLOBAL_TOKEN.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        "server"
    } else {
        "none"
    }
}

/// On a server: may the current tenant use Claude Code right now (not restricted, and a token exists)? On a computer: is it
/// installed?
pub fn usable_here() -> bool {
    if !crate::tenant::multi_tenant() {
        return binary().is_some();
    }
    allowed_here() && server_token().is_some()
}

/// The program, started the way this place needs. On a computer it is the person's own: it inherits what they have (their login),
/// minus the server's variables. On a server it gets an almost empty environment: PATH, a private HOME inside the tenant's own
/// folder (so its sessions are theirs alone and a redeploy wipes them), the tenant's own token, and nothing else of the server's
/// (no Firebase key, no server secret, no other person's token); it works in the tenant's data folder. The returned token is what
/// to cut out of any error text.
fn command(bin: PathBuf) -> Result<(Command, Option<String>), String> {
    if !crate::tenant::multi_tenant() {
        let mut cmd = Command::new(bin);
        crate::exec::scrub_server(&mut cmd);
        return Ok((cmd, None));
    }
    let t = crate::tenant::current().filter(|_| allowed_here()).ok_or("Claude Code is not available to this account.")?;
    let token = server_token().ok_or(NO_TOKEN)?;
    let home = t.config.join("claude-home");
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&home).map_err(|e| format!("Could not prepare Claude Code's folder: {e}"))?;
        let _ = std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700));
    }
    let _ = std::fs::create_dir_all(&t.data);
    let mut cmd = Command::new(bin);
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".into()))
        .env("HOME", &home)
        .env("CLAUDE_CODE_OAUTH_TOKEN", &token)
        .env("DISABLE_AUTOUPDATER", "1")
        .current_dir(&t.data);
    Ok((cmd, Some(token)))
}

/// An error text as the person may see it: never with the token in it, and on a server a refused token is explained.
fn present_error(e: String, token: Option<&str>) -> String {
    let Some(token) = token else { return e };
    let e = e.replace(token, "[token]");
    let low = e.to_lowercase();
    if ["401", "unauthorized", "authentication", "invalid token", "invalid bearer", "oauth token", "token has expired", "expired", "not logged in", "/login", "invalid x-api-key", "revoked"].iter().any(|w| low.contains(w)) {
        return TOKEN_REJECTED.into();
    }
    e
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

/// Send one message. `session` is remembered between calls so the conversation continues. If the stored session is gone (on a
/// server the session files are lost at a redeploy or sleep) and nothing came back, it is dropped and the message is sent again
/// as a new session.
pub fn run(model: &str, effort: Option<&str>, session: &mut Option<String>, prompt: &str, system_extra: &str, cancel: &Arc<AtomicBool>, on_event: &mut dyn FnMut(Event)) -> Result<(), String> {
    run_in_mode(None, model, effort, session, prompt, system_extra, cancel, on_event)
}

/// `run`, with a permission mode for this question only (None: the saved mode, as the tools server reads it).
#[allow(clippy::too_many_arguments)]
pub fn run_in_mode(mode: Option<crate::policy::Mode>, model: &str, effort: Option<&str>, session: &mut Option<String>, prompt: &str, system_extra: &str, cancel: &Arc<AtomicBool>, on_event: &mut dyn FnMut(Event)) -> Result<(), String> {
    if crate::tenant::is_restricted() && !allowed_here() {
        return Err("Claude Code is not available to this account.".into());
    }
    let had_session = session.is_some();
    let mut got_event = false;
    let r = run_once(mode, model, effort, session, prompt, system_extra, cancel, &mut |e| {
        got_event = true;
        on_event(e)
    });
    match r {
        Err(ref e) if had_session && !got_event && e != super::agent::STOPPED && !cancel.load(Ordering::Relaxed) => {
            *session = None;
            run_once(mode, model, effort, session, prompt, system_extra, cancel, on_event)
        }
        r => r,
    }
}

/// The environment of the tools program Claude Code starts. It works for the same tenant as this request. The claude program itself
/// is scrubbed of the server's variables (see `exec::scrub_server`), so the tools program is told here what it needs to find the tenant.
fn tools_env(tenant: Option<&crate::tenant::Tenant>, mode: Option<crate::policy::Mode>, root: Option<String>) -> Value {
    let mut env = json!({ "ZO_MCP_EXTERNAL": "1" });
    if let Some(t) = tenant {
        env["ZO_TENANT_ID"] = json!(t.id);
        env["ZO_MULTI_TENANT"] = json!("1");
        // the tools program of a restricted person is restricted too: web, memory and questions only (fails closed: see tenant::current)
        if t.restricted {
            env["ZO_TENANT_RESTRICTED"] = json!("1");
        }
        if let Some(root) = root {
            env["ZO_TENANT_ROOT"] = json!(root);
        }
    }
    if let Some(m) = mode {
        env["ZO_MCP_MODE"] = json!(m.name());
    }
    env
}

#[allow(clippy::too_many_arguments)]
fn run_once(mode: Option<crate::policy::Mode>, model: &str, effort: Option<&str>, session: &mut Option<String>, prompt: &str, system_extra: &str, cancel: &Arc<AtomicBool>, on_event: &mut dyn FnMut(Event)) -> Result<(), String> {
    let base = super::agent::system_prompt();
    let system = if system_extra.trim().is_empty() { base.to_string() } else { format!("{base}\n\n{system_extra}") };
    // Claude can look at what the tools return as pictures (a screenshot comes back as an image)
    let system = format!("{system}\n\n{}", super::agent::VISION_YES);
    let bin = binary().ok_or("Claude Code is not installed. Install it (npm install -g @anthropic-ai/claude-code), then sign in with: claude auth login")?;
    let env = tools_env(crate::tenant::current().as_ref(), mode, std::env::var("ZO_TENANT_ROOT").ok());
    let config = json!({ "mcpServers": { "zohara": { "command": mcp_binary(), "env": env } } }).to_string();
    let (mut cmd, token) = command(bin)?;
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
        crate::tenant::spawn(move || {
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
        (Some(e), _) => Err(present_error(e, token.as_deref())),
        (None, true) => Ok(()),
        (None, false) => {
            let why: String = stderr.trim().chars().take(300).collect();
            let hint = if token.is_some() { "" } else { ". If you are not signed in, run: claude auth login" };
            Err(present_error(format!("Claude Code stopped with an error{}{hint}", if why.is_empty() { String::new() } else { format!(": {why}") }), token.as_deref()))
        }
    }
}

/// One plain question to Claude Code, no tools and nothing saved: the answer as text. Used for small background jobs such as the
/// memory agent.
pub fn complete(model: &str, system: &str, prompt: &str, cancel: &Arc<AtomicBool>) -> Result<String, String> {
    let bin = binary().ok_or("Claude Code is not installed.")?;
    let (mut cmd, token) = command(bin)?;
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
        crate::tenant::spawn(move || {
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
        Some(e) => Err(present_error(e, token.as_deref())),
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
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

#[cfg(test)]
mod env_tests {
    use super::*;

    #[test]
    fn the_tools_program_of_an_admin_is_told_which_tenant_and_where_the_folders_are() {
        let t = crate::tenant::Tenant { id: "a".repeat(32), config: "/c".into(), data: "/d".into(), restricted: false };
        let e = tools_env(Some(&t), Some(crate::policy::Mode::Bypass), Some("/roots".into()));
        assert_eq!((e["ZO_TENANT_ID"].as_str(), e["ZO_MULTI_TENANT"].as_str(), e["ZO_TENANT_ROOT"].as_str(), e["ZO_MCP_MODE"].as_str().is_some()), (Some("a".repeat(32).as_str()), Some("1"), Some("/roots"), true));
        let desktop = tools_env(None, None, Some("/roots".into()));
        assert!(desktop.get("ZO_TENANT_ID").is_none() && desktop.get("ZO_TENANT_ROOT").is_none(), "no tenant, nothing about tenants");
    }
}

#[cfg(test)]
mod server_tests {
    use super::*;
    use crate::tenant::testkit::{env_once, fresh, SECRET};
    use crate::tenant::{scope, test_multi};
    use std::collections::HashMap;

    const TOKEN_A: &str = "sk-ant-oat01-AAAA-tenant-a-token-0123456789abcdef";
    const TOKEN_B: &str = "sk-ant-oat01-BBBB-tenant-b-token-0123456789abcdef";
    const GLOBAL: &str = "sk-ant-oat01-GLOBAL-fallback-token-0123456789abc";
    const FIREBASE: &str = "firebase-service-account-secret-value";

    /// A stand-in `claude` that records its environment, arguments and folder in `dir`, then prints a short stream (or fails).
    fn fake(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let script = dir.join("claude");
        std::fs::write(&script, format!("#!/bin/sh\nenv > {d}/env\nprintf '%s\\n' \"$@\" > {d}/args\npwd > {d}/pwd\n{body}\n", d = dir.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    const OK: &str = "echo '{\"type\":\"system\",\"session_id\":\"s1\"}'; echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}'";

    fn ask() -> Result<String, String> {
        let (mut session, mut text) = (None, String::new());
        run_in_mode(None, "sonnet", None, &mut session, "hello", "", &Arc::new(AtomicBool::new(false)), &mut |e| if let Event::Text(t) = e { text.push_str(&t) }).map(|_| text)
    }

    fn setup() -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        let g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        env_once();
        test_multi(true);
        std::env::set_var("FIREBASE_SERVICE_ACCOUNT", FIREBASE);
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", GLOBAL);
        std::env::set_var("ZOAI_KEY_OPENAI", "sk-openai-key-of-the-server");
        let dir = std::env::temp_dir().join(format!("zo-fake-claude-srv-{}-{}", std::process::id(), std::thread::current().name().unwrap_or("t").replace("::", "_")));
        let _ = std::fs::remove_dir_all(&dir);
        (g, dir)
    }

    fn cleanup(dir: &std::path::Path) {
        for k in ["FIREBASE_SERVICE_ACCOUNT", "CLAUDE_CODE_OAUTH_TOKEN", "ZOAI_KEY_OPENAI", "ZOAI_CLAUDE_BIN"] {
            std::env::remove_var(k);
        }
        test_multi(false);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn dumped_env(dir: &std::path::Path) -> HashMap<String, String> {
        std::fs::read_to_string(dir.join("env")).unwrap().lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn on_a_server_claude_gets_a_minimal_environment_with_only_this_tenants_token_a_private_home_and_no_built_in_tools() {
        let (_g, dir) = setup();
        let (a, b) = (fresh(false), fresh(false));
        std::env::set_var("ZOAI_CLAUDE_BIN", fake(&dir, OK));
        scope(&b, || store_token(TOKEN_B).unwrap());
        scope(&a, || store_token(TOKEN_A).unwrap());
        assert_eq!(scope(&a, ask).unwrap(), "ok");
        let env = dumped_env(&dir);
        // nothing beyond what the program needs (and what the shell itself adds)
        for k in env.keys() {
            assert!(["PATH", "HOME", "CLAUDE_CODE_OAUTH_TOKEN", "DISABLE_AUTOUPDATER", "PWD", "OLDPWD", "SHLVL", "_"].contains(&k.as_str()), "unexpected variable {k}");
        }
        assert_eq!(env["CLAUDE_CODE_OAUTH_TOKEN"], TOKEN_A, "this tenant's own token, not the server's and not another person's");
        let all = std::fs::read_to_string(dir.join("env")).unwrap();
        for secret in [FIREBASE, SECRET, GLOBAL, TOKEN_B, "sk-openai-key-of-the-server"] {
            assert!(!all.contains(secret), "the environment shows {secret}");
        }
        // an isolated home inside the tenant's own config folder, private
        let home = a.config.join("claude-home");
        assert_eq!(env["HOME"], home.to_string_lossy());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&home).unwrap().permissions().mode() & 0o777, 0o700);
        // it works in the tenant's data folder
        assert_eq!(std::fs::canonicalize(std::fs::read_to_string(dir.join("pwd")).unwrap().trim()).unwrap(), std::fs::canonicalize(&a.data).unwrap());
        // Claude's own tools are off, ZO-AI's tools only
        let args: Vec<String> = std::fs::read_to_string(dir.join("args")).unwrap().lines().map(str::to_string).collect();
        let i = args.iter().position(|x| x == "--tools").expect("--tools is passed");
        assert_eq!(args[i + 1], "", "the list of built-in tools is empty");
        let j = args.iter().position(|x| x == "--allowedTools").unwrap();
        assert_eq!(args[j + 1], "mcp__zohara__*");
        assert!(args.contains(&"--strict-mcp-config".to_string()));
        // the same holds for the small background calls
        let _ = scope(&a, || complete("haiku", "s", "p", &Arc::new(AtomicBool::new(false))));
        assert_eq!(dumped_env(&dir)["CLAUDE_CODE_OAUTH_TOKEN"], TOKEN_A);
        cleanup(&dir);
    }

    #[test]
    fn without_an_own_token_an_admin_uses_the_servers_fallback_only_if_there_is_one_and_a_restricted_tenant_never_gets_claude() {
        let (_g, dir) = setup();
        let (admin, restricted) = (fresh(false), fresh(true));
        std::env::set_var("ZOAI_CLAUDE_BIN", fake(&dir, OK));
        // nothing at all
        assert_eq!(scope(&admin, ask).unwrap_err(), NO_TOKEN);
        assert!(!scope(&admin, usable_here));
        // the fallback
        set_global_token(Some(GLOBAL.into()));
        assert!(scope(&admin, usable_here));
        assert_eq!(scope(&admin, ask).unwrap(), "ok");
        assert_eq!(dumped_env(&dir)["CLAUDE_CODE_OAUTH_TOKEN"], GLOBAL);
        // a restricted tenant: not even with a token stored by force in its key store, not with the fallback
        std::fs::remove_file(dir.join("env")).unwrap();
        scope(&restricted, || {
            assert!(store_token(TOKEN_A).is_err(), "a restricted tenant cannot store a token");
            crate::ai::keys::set(TOKEN_KEY, TOKEN_A).unwrap(); // stored behind the check's back
            assert!(!usable_here() && binary().is_none());
            assert!(ask().unwrap_err().contains("not available"));
            assert!(complete("haiku", "s", "p", &Arc::new(AtomicBool::new(false))).is_err());
            assert!(command(PathBuf::from("claude")).is_err());
        });
        assert!(!dir.join("env").exists(), "claude was never started for the restricted tenant");
        set_global_token(None);
        cleanup(&dir);
    }

    #[test]
    fn a_refused_token_is_explained_and_the_token_never_shows_in_an_error() {
        let (_g, dir) = setup();
        let a = fresh(false);
        scope(&a, || store_token(TOKEN_A).unwrap());
        std::env::set_var("ZOAI_CLAUDE_BIN", fake(&dir, "echo '{\"type\":\"result\",\"is_error\":true,\"result\":\"API Error: 401 authentication_error invalid\"}'"));
        assert_eq!(scope(&a, ask).unwrap_err(), TOKEN_REJECTED);
        // an error that quotes the token: cut out; other errors pass through
        std::env::set_var("ZOAI_CLAUDE_BIN", fake(&dir, &format!("echo 'boom with {TOKEN_A} inside' >&2; exit 3")));
        let e = scope(&a, ask).unwrap_err();
        assert!(!e.contains(TOKEN_A) && e.contains("stopped with an error"), "{e}");
        assert_eq!(present_error("The model is overloaded".into(), Some(TOKEN_A)), "The model is overloaded");
        assert_eq!(present_error("401 nope".into(), None), "401 nope", "on a computer the message is left alone");
        cleanup(&dir);
    }

    #[test]
    fn the_token_is_stored_encrypted_checked_for_shape_and_removable() {
        let (_g, dir) = setup();
        let a = fresh(false);
        scope(&a, || {
            assert!(token_set_at().is_none());
            for bad in ["", "hello", "sk-ant-short", "sk-ant-oat01-has space in the middle of the token 0123456789", &format!("sk-ant-{}", "x".repeat(500))] {
                assert!(store_token(bad).is_err(), "{bad}");
            }
            let at = store_token(&format!("  {TOKEN_A}\n")).unwrap();
            assert_eq!(token_set_at(), Some(at));
            assert_eq!(crate::ai::keys::get(TOKEN_KEY).as_deref(), Some(TOKEN_A));
            let raw = std::fs::read(a.config.join(crate::ai::keys::ENC_FILE)).unwrap();
            assert!(!String::from_utf8_lossy(&raw).contains("sk-ant"), "the file holds ciphertext only");
            assert!(crate::ai::keys::decrypt_map(SECRET, &a.id, &raw).unwrap().contains_key(TOKEN_KEY));
            assert!(crate::ai::keys::decrypt_map(SECRET, &"0".repeat(32), &raw).is_err(), "another tenant's key does not open it");
            clear_token().unwrap();
            assert!(token_set_at().is_none() && crate::ai::keys::get(TOKEN_KEY).is_none());
        });
        cleanup(&dir);
    }

    #[test]
    fn everybody_may_use_claude_code_only_when_the_operator_says_all_and_never_on_the_owners_token() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::tenant::testkit::env_once();
        test_multi(true);
        let (admin, guest) = (crate::tenant::testkit::fresh(false), crate::tenant::testkit::fresh(true));
        const T: &str = "sk-ant-oat01-guest-token-0123456789abcdefghijklmnop";
        set_global_token(Some("sk-ant-oat01-owners-fallback-token-0123456789abcdef".into()));
        // default (admins): the guest has nothing, and cannot store a token
        test_users_all(Some(false));
        scope(&guest, || {
            assert!(!allowed_here() && !usable_here() && binary().is_none());
            assert!(store_token(T).is_err());
            assert_eq!(token_source(), "none");
        });
        scope(&admin, || {
            assert!(allowed_here());
            assert_eq!(token_source(), "server", "the admin may use the operator's fallback token");
        });
        // all: the guest may keep a token of their own and use it; the owner's fallback never becomes theirs
        test_users_all(Some(true));
        scope(&guest, || {
            assert!(allowed_here());
            assert_eq!(token_source(), "none", "no own token: not the owner's subscription");
            assert!(!usable_here());
            assert!(server_token().is_none());
            store_token(T).unwrap();
            assert_eq!((token_source(), usable_here()), ("own", true));
            assert_eq!(server_token().as_deref(), Some(T));
            clear_token().unwrap();
        });
        // the tools program of a guest is told that it is restricted; the admin's is not
        let env = tools_env(Some(&guest), None, Some("/r".into()));
        assert_eq!(env["ZO_TENANT_RESTRICTED"], "1");
        assert!(tools_env(Some(&admin), None, Some("/r".into())).get("ZO_TENANT_RESTRICTED").is_none());
        test_users_all(None);
        set_global_token(None);
        test_multi(false);
    }
}
