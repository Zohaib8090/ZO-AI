//! ZO-AI as an MCP client: other MCP servers (a GitHub server, a database server, …) added in Settings. Their tools join the ones
//! the chat AI can use, and every call asks first (they are other people's programs), unless the mode is Bypass.

use crate::approval::Approver;
use crate::policy::{decide, Decision, Mode, Risk};
use crate::{audit, mcp};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub name: String,
    /// The program and its arguments, as typed in Settings.
    pub command: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

fn file() -> std::path::PathBuf {
    audit::config_dir().join("mcp-servers.json")
}

pub fn load_servers() -> Vec<ServerConfig> {
    std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_servers(all: &[ServerConfig]) {
    let _ = std::fs::create_dir_all(audit::config_dir());
    let tmp = file().with_extension("json.tmp");
    if let Ok(s) = serde_json::to_string_pretty(all) {
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, file());
        }
    }
}

/// A name that is safe in a tool name: letters, digits, `_` and `-`.
pub fn clean_name(s: &str) -> String {
    let n: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    n.trim_matches('_').to_string()
}

/// Splits a typed command into words; single and double quotes keep words together.
pub fn split_command(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub struct Tool {
    pub server: String,
    /// The name as the server knows it.
    pub name: String,
    pub description: String,
    pub schema: Value,
}

impl Tool {
    /// The name the model sees: `server__tool`, cut to what providers allow.
    pub fn public_name(&self) -> String {
        let n = format!("{}__{}", clean_name(&self.server), clean_name(&self.name));
        n.chars().take(64).collect()
    }
}

pub struct Connection {
    pub server: String,
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Connection {
    /// Starts the server, shakes hands and returns it ready for use.
    pub fn start(cfg: &ServerConfig) -> Result<Connection, String> {
        let words = split_command(&cfg.command);
        let (program, args) = words.split_first().ok_or("The command is empty.")?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start \"{program}\": {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        let mut c = Connection { server: cfg.name.clone(), child, stdin, rx, next_id: 1 };
        c.request("initialize", json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "zo-ai", "version": env!("CARGO_PKG_VERSION") },
        }), Duration::from_secs(20))?;
        c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        Ok(c)
    }

    fn send(&mut self, v: &Value) -> Result<(), String> {
        writeln!(self.stdin, "{v}").and_then(|_| self.stdin.flush()).map_err(|e| format!("The server closed: {e}"))
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(v) if v["id"] == json!(id) => {
                    if let Some(e) = v.get("error") {
                        return Err(e["message"].as_str().unwrap_or("The server reported an error.").to_string());
                    }
                    return Ok(v["result"].clone());
                }
                Ok(_) => {} // a notification or a log line
                Err(RecvTimeoutError::Timeout) => return Err(format!("The server did not answer {method} in time.")),
                Err(RecvTimeoutError::Disconnected) => return Err("The server stopped.".into()),
            }
        }
    }

    pub fn tools(&mut self) -> Result<Vec<Tool>, String> {
        let r = self.request("tools/list", json!({}), Duration::from_secs(20))?;
        Ok(r["tools"].as_array().into_iter().flatten().filter_map(|t| {
            Some(Tool {
                server: self.server.clone(),
                name: t["name"].as_str()?.to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                schema: t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            })
        }).collect())
    }

    pub fn call(&mut self, tool: &str, args: &Value) -> Result<String, String> {
        let r = self.request("tools/call", json!({ "name": tool, "arguments": args }), Duration::from_secs(120))?;
        let text = r["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
        if r["isError"] == true { Err(text) } else { Ok(text) }
    }
}

/// Every enabled server that could be started, for the length of one conversation turn. Dropping it stops them.
#[derive(Default)]
pub struct External {
    conns: Vec<Connection>,
    tools: Vec<Tool>,
    /// What went wrong with servers that did not start.
    pub problems: Vec<String>,
}

impl External {
    pub fn start() -> External {
        Self::start_with(&load_servers())
    }

    pub fn start_with(servers: &[ServerConfig]) -> External {
        let mut e = External::default();
        for cfg in servers.iter().filter(|s| s.enabled) {
            match Connection::start(cfg).and_then(|mut c| c.tools().map(|t| (c, t))) {
                Ok((c, tools)) => {
                    e.tools.extend(tools);
                    e.conns.push(c);
                }
                Err(err) => e.problems.push(format!("{}: {err}", cfg.name)),
            }
        }
        e
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub fn owns(&self, public_name: &str) -> bool {
        self.tools.iter().any(|t| t.public_name() == public_name)
    }

    /// Runs one of their tools, asking first (these are other people's programs).
    pub fn execute(&mut self, public_name: &str, args: &Value, mode: Mode, approver: &dyn Approver) -> Result<String, String> {
        let tool = self.tools.iter().find(|t| t.public_name() == public_name).ok_or_else(|| format!("unknown tool: {public_name}"))?;
        let (server, name) = (tool.server.clone(), tool.name.clone());
        if decide(mode, Risk::Change, None) == Decision::Ask && !approver.approve(public_name, &format!("{}\nfrom the MCP server \"{server}\"", mcp::summarize(&name, args))) {
            audit::log(public_name, args, "denied");
            return Err("The person did not approve this (or ZO-AI is not running to ask). Nothing was done.".into());
        }
        let conn = self.conns.iter_mut().find(|c| c.server == server).ok_or("The server is not running.")?;
        let r = conn.call(&name, args);
        audit::log(public_name, args, if r.is_ok() { "ok" } else { "error" });
        r
    }
}

/// Starts a server just to see that it works: its tool names, or what went wrong.
pub fn test(cfg: &ServerConfig) -> Result<Vec<String>, String> {
    let mut c = Connection::start(cfg)?;
    Ok(c.tools()?.into_iter().map(|t| t.name).collect())
}

/// A stand-in MCP server written in shell: it answers initialize, tools/list and tools/call.
#[cfg(test)]
pub(crate) fn fake_server() -> (std::path::PathBuf, ServerConfig) {
    let dir = std::env::temp_dir().join(format!("zo-fake-mcp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("server.sh");
    std::fs::write(&script, r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) echo '{"jsonrpc":"2.0","id":'$id',"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}';;
    *'"method":"tools/list"'*) echo '{"jsonrpc":"2.0","method":"notifications/message","params":{}}'; echo '{"jsonrpc":"2.0","id":'$id',"result":{"tools":[{"name":"echo","description":"Echo back","inputSchema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}]}}';;
    *'"method":"tools/call"'*) echo '{"jsonrpc":"2.0","id":'$id',"result":{"content":[{"type":"text","text":"echoed"}],"isError":false}}';;
  esac
done
"#).unwrap();
    let cfg = ServerConfig { name: "my fake!".into(), command: format!("sh {}", script.display()), enabled: true };
    (dir, cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::Deny;

    use super::fake_server;

    struct Yes;
    impl Approver for Yes {
        fn approve(&self, _: &str, _: &str) -> bool {
            true
        }
    }

    #[test]
    fn a_server_is_started_listed_and_called_and_asks_first() {
        let (dir, cfg) = fake_server();
        assert_eq!(test(&cfg).unwrap(), ["echo"]);

        let mut ext = External::start_with(&[cfg.clone()]);
        assert!(ext.problems.is_empty(), "{:?}", ext.problems);
        let t = &ext.tools()[0];
        assert_eq!(t.public_name(), "my_fake__echo");
        assert!(ext.owns("my_fake__echo") && !ext.owns("echo"));
        assert_eq!(ext.tools()[0].schema["required"][0], "text");

        let args = json!({ "text": "hi" });
        assert!(ext.execute("my_fake__echo", &args, Mode::Auto, &Deny).unwrap_err().contains("did not approve"));
        assert_eq!(ext.execute("my_fake__echo", &args, Mode::Auto, &Yes).unwrap(), "echoed");
        assert_eq!(ext.execute("my_fake__echo", &args, Mode::Bypass, &Deny).unwrap(), "echoed", "bypass never asks");
        drop(ext);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_server_that_cannot_start_is_reported_not_fatal() {
        let bad = ServerConfig { name: "nope".into(), command: "/definitely/not/a/program".into(), enabled: true };
        let off = ServerConfig { name: "off".into(), command: "/definitely/not/a/program".into(), enabled: false };
        let ext = External::start_with(&[bad, off]);
        assert_eq!(ext.problems.len(), 1);
        assert!(ext.problems[0].starts_with("nope:"));
        assert!(ext.tools().is_empty());
        assert!(test(&ServerConfig { name: "x".into(), command: "".into(), enabled: true }).is_err());
    }

    #[test]
    fn commands_are_split_like_a_shell_would_and_names_are_cleaned() {
        assert_eq!(split_command(r#"npx -y @scope/server "/path with space" 'a b'"#), ["npx", "-y", "@scope/server", "/path with space", "a b"]);
        assert_eq!(split_command("  one   two  "), ["one", "two"]);
        assert_eq!(clean_name("my fake!"), "my_fake");
        assert_eq!(clean_name("github-server"), "github-server");
    }
}
