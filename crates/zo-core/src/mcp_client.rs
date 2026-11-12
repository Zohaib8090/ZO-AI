//! ZO-AI as an MCP client: other MCP servers (a GitHub server, a database server, …) added in Settings. Their tools join the ones
//! the chat AI can use, and every call asks first (they are other people's programs), unless the mode is Bypass.

use crate::approval::Approver;
use crate::policy::{decide, Decision, Mode, Risk};
use crate::{audit, mcp};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct ServerConfig {
    pub name: String,
    /// The program and its arguments, as typed in Settings, or the `https://` address of a remote server.
    pub command: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Environment variables for a program that is started here (API keys and the like).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Extra headers sent to a remote server on every request (`Authorization: Bearer …`, `X-API-Key: …`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// For a remote server that has no automatic sign-in registration: the client id (and secret) you registered for ZO-AI there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// The local port the sign-in comes back to (the address is http://127.0.0.1:PORT/callback); a fixed one can be registered at the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_port: Option<u16>,
    /// Scopes to ask for at sign-in, instead of the ones the server suggests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
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
        // the file can hold keys (environment variables, headers), so only the owner may read it
        let written = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).and_then(|mut f| f.write_all(s.as_bytes()))
        };
        if written.is_ok() {
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

/// The opposite of `split_command`: a program and its arguments as one line, quoting what has spaces.
pub fn join_command(program: &str, args: &[String]) -> String {
    let q = |w: &str| if w.is_empty() || w.contains(char::is_whitespace) || w.contains('"') || w.contains('\'') { format!("'{}'", w.replace('\'', "'\"'\"'")) } else { w.to_string() };
    std::iter::once(program.to_string()).chain(args.iter().cloned()).map(|w| q(&w)).collect::<Vec<_>>().join(" ")
}

/// Reads a server configuration the way other apps write it. Accepts Claude Desktop's `{"mcpServers": {...}}` (and Claude Code's
/// `.mcp.json`, Cursor's, VS Code's `{"servers": {...}}`), a bare `{"name": {...}}`, or one server `{"command": ..}` / `{"url": ..}`.
/// A server is a program (`command`, `args`, `env`) or a remote one (`url`, `headers`).
pub fn import_json(text: &str) -> Result<Vec<ServerConfig>, String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("That is not valid JSON: {e}"))?;
    let map = v.get("mcpServers").or_else(|| v.get("servers")).or_else(|| v.get("mcp")).cloned().unwrap_or_else(|| v.clone());
    let one = |name: &str, d: &Value| -> Option<ServerConfig> {
        let strings = |k: &str| -> BTreeMap<String, String> { d[k].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default() };
        let url = d["url"].as_str().or_else(|| d["serverUrl"].as_str()).or_else(|| d["httpUrl"].as_str());
        let command = match (url, d["command"].as_str()) {
            (Some(u), _) => u.to_string(),
            (None, Some(c)) => {
                let args: Vec<String> = d["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                join_command(c, &args)
            }
            _ => return None,
        };
        let oauth = &d["oauth"];
        Some(ServerConfig {
            name: clean_name(name),
            command,
            enabled: !d["disabled"].as_bool().unwrap_or(false),
            env: strings("env"),
            headers: strings("headers"),
            client_id: oauth["clientId"].as_str().or_else(|| d["clientId"].as_str()).map(str::to_string),
            client_secret: oauth["clientSecret"].as_str().or_else(|| d["clientSecret"].as_str()).map(str::to_string),
            callback_port: oauth["callbackPort"].as_u64().or_else(|| d["callbackPort"].as_u64()).and_then(|p| u16::try_from(p).ok()),
            scope: oauth["scopes"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")).or_else(|| oauth["scope"].as_str().map(str::to_string)),
        })
    };
    let mut out = Vec::new();
    if map.get("command").is_some() || map.get("url").is_some() || map.get("serverUrl").is_some() {
        out.extend(one("server", &map));
    } else if let Some(o) = map.as_object() {
        out.extend(o.iter().filter_map(|(n, d)| one(n, d)));
    }
    out.retain(|c| !c.name.is_empty());
    if out.is_empty() {
        return Err("No server was found in that. A server needs a \"command\" (a program) or a \"url\" (a remote server).".into());
    }
    Ok(out)
}

pub struct Tool {
    pub server: String,
    /// The name as the server knows it.
    pub name: String,
    pub description: String,
    pub schema: Value,
    /// The server says this tool only reads (`annotations.readOnlyHint`): it runs without asking in Auto mode.
    pub read_only: bool,
}

impl Tool {
    /// The name the model sees: `server__tool`, cut to what providers allow.
    pub fn public_name(&self) -> String {
        let n = format!("{}__{}", clean_name(&self.server), clean_name(&self.name));
        n.chars().take(64).collect()
    }
}

/// How a server is reached: a program started here and spoken to over its standard input and output, or a remote server
/// reached over HTTP (an `https://` address, the "streamable HTTP" transport).
enum Transport {
    Stdio { child: Child, stdin: ChildStdin, rx: Receiver<Value> },
    Http { url: String, agent: ureq::Agent, token: Option<String>, saved: Option<crate::mcp_oauth::Saved>, session: Option<String>, headers: BTreeMap<String, String>, protocol: Option<String> },
}

pub struct Connection {
    pub server: String,
    t: Transport,
    next_id: u64,
    /// What the server says about itself when the connection starts: guidance for the model, and what it offers beyond tools.
    pub instructions: Option<String>,
    caps: Value,
}

impl Drop for Connection {
    fn drop(&mut self) {
        match &mut self.t {
            Transport::Stdio { child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
            }
            // a server that gave us a session is told it is over (best effort)
            Transport::Http { url, session: Some(sid), token, agent, .. } => {
                let mut req = agent.delete(url).timeout(Duration::from_secs(2)).set("Mcp-Session-Id", sid);
                if let Some(t) = token.as_deref() {
                    req = req.set("Authorization", &format!("Bearer {t}"));
                }
                let _ = req.call();
            }
            _ => {}
        }
    }
}

/// The start of the error for a session the server no longer knows (the request is then repeated on a new one).
const SESSION_GONE: &str = "The server's session ended";

/// The start of the error for a server that has to be signed in to (callers look for it, to offer the sign-in).
pub const NEEDS_SIGN_IN: &str = "The server wants you to sign in first";

/// Is the "command" really the address of a remote server?
pub fn is_url(command: &str) -> bool {
    let c = command.trim();
    c.starts_with("https://") || c.starts_with("http://")
}

/// The keyring id under which a remote server's access token is kept.
pub fn token_id(server: &str) -> String {
    format!("mcp-{}", clean_name(server))
}

/// The reply of a streamable-HTTP server is either one JSON document or an event stream; this finds the answer to `id`.
fn answer_in(body: &str, is_stream: bool, id: u64) -> Option<Value> {
    let matches = |v: &Value| v["id"] == json!(id);
    if is_stream {
        let mut data = String::new();
        let mut found = None;
        for line in body.lines().chain(std::iter::once("")) {
            if let Some(d) = line.strip_prefix("data:") {
                data.push_str(d.trim_start());
            } else if line.trim().is_empty() && !data.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(&data) {
                    if matches(&v) {
                        found = Some(v);
                    }
                }
                data.clear();
            }
        }
        found
    } else {
        let v: Value = serde_json::from_str(body).ok()?;
        match v {
            Value::Array(items) => items.into_iter().find(matches),
            v if matches(&v) => Some(v),
            _ => None,
        }
    }
}

impl Connection {
    /// Starts (or connects to) the server, shakes hands and returns it ready for use.
    pub fn start(cfg: &ServerConfig) -> Result<Connection, String> {
        if crate::tenant::is_restricted() {
            return Err("MCP servers are not available to this account.".into());
        }
        if is_url(&cfg.command) {
            return Self::connect_http(cfg);
        }
        let words = split_command(&cfg.command);
        let (program, args) = words.split_first().ok_or("The command is empty.")?;
        let mut cmd = Command::new(program);
        crate::exec::scrub(&mut cmd);
        let mut child = cmd
            .args(args)
            .envs(&cfg.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start \"{program}\": {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = channel();
        crate::tenant::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        let mut c = Connection { server: cfg.name.clone(), t: Transport::Stdio { child, stdin, rx }, next_id: 1, instructions: None, caps: Value::Null };
        c.handshake()?;
        Ok(c)
    }

    fn connect_http(cfg: &ServerConfig) -> Result<Connection, String> {
        let url = cfg.command.trim().to_string();
        // what the keyring holds is either a token the person pasted or the record a sign-in made
        let stored = crate::ai::keys::get(&token_id(&cfg.name));
        let mut saved = stored.as_deref().and_then(crate::mcp_oauth::Saved::parse);
        // an access token that is about to run out is renewed first, so the first request does not fail
        if let Some(fresh) = saved.as_ref().filter(|s| s.nearly_expired() && s.refresh.is_some()).and_then(|s| crate::mcp_oauth::refresh(&cfg.name, s).ok()) {
            saved = Some(fresh);
        }
        let token = saved.as_ref().map(|s| s.access.clone()).or(stored);
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).build();
        let mk = |url: String| Connection { server: cfg.name.clone(), t: Transport::Http { url, agent: agent.clone(), token: token.clone(), saved: saved.clone(), session: None, headers: cfg.headers.clone(), protocol: None }, next_id: 1, instructions: None, caps: Value::Null };
        let mut c = mk(url.clone());
        match c.handshake() {
            Ok(()) => Ok(c),
            // The address of the site itself (https://name.example.com) instead of its MCP address (…/mcp): try that too.
            Err(first) if url.trim_end_matches('/').matches('/').count() == 2 => {
                let mut c2 = mk(format!("{}/mcp", url.trim_end_matches('/')));
                // if the site's own address says "no" (404, 405) but its /mcp address answers, what /mcp says is the real answer
                // (a sign-in is wanted, for one); if the site could not be reached at all, that is what to report
                c2.handshake().map(|_| c2).map_err(|second| if first.starts_with("Could not reach") { first } else { second })
            }
            Err(e) => Err(e),
        }
    }

    fn handshake(&mut self) -> Result<(), String> {
        let r = self.request("initialize", json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "zo-ai", "version": env!("CARGO_PKG_VERSION") },
        }), Duration::from_secs(20))?;
        self.instructions = r["instructions"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        self.caps = r["capabilities"].clone();
        // from now on every request names the protocol version the server agreed to
        if let (Transport::Http { protocol, .. }, Some(v)) = (&mut self.t, r["protocolVersion"].as_str()) {
            *protocol = Some(v.to_string());
        }
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
    }

    /// One message to a remote server; returns its reply body (empty when it only acknowledges) and whether that is a stream.
    fn http_post(&mut self, body: &Value, timeout: Duration) -> Result<(String, bool), String> {
        let server = self.server.clone();
        let Transport::Http { url, agent, token, saved, session, headers, protocol } = &mut self.t else { return Err("not a remote server".into()) };
        let own_auth = headers.keys().any(|k| k.eq_ignore_ascii_case("authorization"));
        let send = |token: &Option<String>, session: &Option<String>| {
            let mut req = agent.post(url).timeout(timeout).set("Accept", "application/json, text/event-stream").set("Content-Type", "application/json");
            for (k, v) in headers.iter() {
                req = req.set(k, v);
            }
            if let Some(v) = protocol.as_deref() {
                req = req.set("MCP-Protocol-Version", v);
            }
            if let (Some(t), false) = (token.as_deref(), own_auth) {
                req = req.set("Authorization", &format!("Bearer {t}"));
            }
            if let Some(sid) = session.as_deref() {
                req = req.set("Mcp-Session-Id", sid);
            }
            req.send_json(body.clone())
        };
        let mut result = send(token, session);
        // an expired sign-in: use the refresh token once and try again
        if matches!(result, Err(ureq::Error::Status(401, _))) {
            if let Some(next) = saved.as_ref().and_then(|s| crate::mcp_oauth::refresh(&server, s).ok()) {
                *token = Some(next.access.clone());
                *saved = Some(next);
                result = send(token, session);
            }
        }
        match result {
            Ok(resp) => {
                if let Some(sid) = resp.header("mcp-session-id") {
                    *session = Some(sid.to_string());
                }
                let stream = resp.content_type().contains("text/event-stream");
                Ok((resp.into_string().unwrap_or_default(), stream))
            }
            Err(ureq::Error::Status(401 | 403, _)) => Err(if token.is_some() {
                "The server refused the access token.".to_string()
            } else {
                format!("{NEEDS_SIGN_IN}.")
            }),
            Err(ureq::Error::Status(404, _)) if session.is_some() => {
                *session = None;
                Err(format!("{SESSION_GONE}."))
            }
            Err(ureq::Error::Status(code, _)) => Err(format!("The server answered with error {code}.")),
            Err(e) => Err(format!("Could not reach the server: {e}")),
        }
    }

    fn send(&mut self, v: &Value) -> Result<(), String> {
        match &mut self.t {
            Transport::Stdio { stdin, .. } => writeln!(stdin, "{v}").and_then(|_| stdin.flush()).map_err(|e| format!("The server closed: {e}")),
            Transport::Http { .. } => self.http_post(v, Duration::from_secs(20)).map(|_| ()),
        }
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if matches!(self.t, Transport::Http { .. }) {
            let (body, stream) = match self.http_post(&msg, timeout) {
                Err(e) if e.starts_with(SESSION_GONE) && method != "initialize" => {
                    // the server forgot us (it restarted, or the session timed out): shake hands again and repeat this once
                    self.handshake()?;
                    self.http_post(&msg, timeout)?
                }
                other => other?,
            };
            let v = answer_in(&body, stream, id).ok_or_else(|| format!("The server did not answer {method}."))?;
            return match v.get("error") {
                Some(e) => Err(e["message"].as_str().unwrap_or("The server reported an error.").to_string()),
                None => Ok(v["result"].clone()),
            };
        }
        self.send(&msg)?;
        let Transport::Stdio { rx, .. } = &self.t else { unreachable!() };
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
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

    /// Every page of a list (`tools/list`, `resources/list`, `prompts/list`): the server may hand them out in pieces.
    fn list_all(&mut self, method: &str, key: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..20 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let r = self.request(method, params, Duration::from_secs(20))?;
            out.extend(r[key].as_array().cloned().unwrap_or_default());
            match r["nextCursor"].as_str().filter(|c| !c.is_empty()) {
                Some(c) => cursor = Some(c.to_string()),
                None => break,
            }
        }
        Ok(out)
    }

    pub fn tools(&mut self) -> Result<Vec<Tool>, String> {
        let mut tools: Vec<Tool> = self.list_all("tools/list", "tools")?.into_iter().filter_map(|t| {
            Some(Tool {
                server: self.server.clone(),
                name: t["name"].as_str()?.to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                schema: t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                read_only: t["annotations"]["readOnlyHint"].as_bool().unwrap_or(false),
            })
        }).collect();
        // what a server offers besides tools (files and data it can show, ready-made prompts) is reached through tools of ours
        let extra = |name: &str, description: &str, props: Value, required: &[&str]| Tool {
            server: self.server.clone(),
            name: name.to_string(),
            description: description.to_string(),
            schema: json!({ "type": "object", "properties": props, "required": required }),
            read_only: true,
        };
        if !self.caps["resources"].is_null() {
            tools.push(extra("zo_resources_list", "List the resources (documents, files, data) this server offers, with their addresses (uri).", json!({}), &[]));
            tools.push(extra("zo_resources_read", "Read one resource of this server by its uri.", json!({ "uri": { "type": "string", "description": "The resource uri from the list" } }), &["uri"]));
        }
        if !self.caps["prompts"].is_null() {
            tools.push(extra("zo_prompts_list", "List the ready-made prompts this server offers, with their arguments.", json!({}), &[]));
            tools.push(extra("zo_prompts_get", "Get one of this server's prompts, filled in with arguments.", json!({ "name": { "type": "string" }, "arguments": { "type": "object", "description": "Argument values by name", "additionalProperties": { "type": "string" } } }), &["name"]));
        }
        Ok(tools)
    }

    pub fn call(&mut self, tool: &str, args: &Value) -> Result<String, String> {
        match tool {
            "zo_resources_list" => {
                let mut lines: Vec<String> = self.list_all("resources/list", "resources")?.iter().map(|r| format!("{} | {} | {} | {}", r["uri"].as_str().unwrap_or(""), r["name"].as_str().unwrap_or(""), r["mimeType"].as_str().unwrap_or(""), r["description"].as_str().unwrap_or(""))).collect();
                if let Ok(t) = self.list_all("resources/templates/list", "resourceTemplates") {
                    lines.extend(t.iter().map(|r| format!("{} (a template: fill in the braces) | {} | {} | {}", r["uriTemplate"].as_str().unwrap_or(""), r["name"].as_str().unwrap_or(""), r["mimeType"].as_str().unwrap_or(""), r["description"].as_str().unwrap_or(""))));
                }
                return Ok(if lines.is_empty() { "This server has no resources.".to_string() } else { lines.join("\n") });
            }
            "zo_resources_read" => {
                let uri = args["uri"].as_str().filter(|u| !u.is_empty()).ok_or("missing argument: uri")?;
                let r = self.request("resources/read", json!({ "uri": uri }), Duration::from_secs(60))?;
                let parts: Vec<String> = r["contents"].as_array().into_iter().flatten().map(|c| match c["text"].as_str() {
                    Some(t) => t.to_string(),
                    None => format!("[binary data, {} bytes of base64, {}]", c["blob"].as_str().map_or(0, str::len), c["mimeType"].as_str().unwrap_or("unknown type")),
                }).collect();
                return Ok(parts.join("\n"));
            }
            "zo_prompts_list" => {
                let lines: Vec<String> = self.list_all("prompts/list", "prompts")?.iter().map(|p| {
                    let a: Vec<String> = p["arguments"].as_array().into_iter().flatten().map(|a| format!("{}{}", a["name"].as_str().unwrap_or(""), if a["required"] == true { " (required)" } else { "" })).collect();
                    format!("{} | {} | arguments: {}", p["name"].as_str().unwrap_or(""), p["description"].as_str().unwrap_or(""), if a.is_empty() { "none".to_string() } else { a.join(", ") })
                }).collect();
                return Ok(if lines.is_empty() { "This server has no prompts.".to_string() } else { lines.join("\n") });
            }
            "zo_prompts_get" => {
                let name = args["name"].as_str().filter(|n| !n.is_empty()).ok_or("missing argument: name")?;
                let r = self.request("prompts/get", json!({ "name": name, "arguments": args.get("arguments").cloned().unwrap_or_else(|| json!({})) }), Duration::from_secs(30))?;
                let text: Vec<String> = r["messages"].as_array().into_iter().flatten().map(|m| format!("{}: {}", m["role"].as_str().unwrap_or(""), m["content"]["text"].as_str().map(str::to_string).unwrap_or_else(|| m["content"].to_string()))).collect();
                return Ok(text.join("\n"));
            }
            _ => {}
        }
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
        if crate::tenant::is_restricted() {
            return External::default();
        }
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

    /// A sentence about the servers that could not be reached, for the model to pass on (none when all were).
    pub fn problems_note(&self) -> Option<String> {
        (!self.problems.is_empty()).then(|| format!("These MCP servers could not be reached right now, so their tools are missing: {}. Tell the person, and say what the error asks for (a sign-in, a restart of the server, a wrong address).", self.problems.join("; ")))
    }

    /// What each connected server asks the model to know about it (`instructions` from its handshake): (server, text).
    pub fn instructions(&self) -> Vec<(String, String)> {
        self.conns.iter().filter_map(|c| Some((c.server.clone(), c.instructions.clone()?))).collect()
    }

    pub fn owns(&self, public_name: &str) -> bool {
        self.tools.iter().any(|t| t.public_name() == public_name)
    }

    /// Runs one of their tools, asking first (these are other people's programs).
    pub fn execute(&mut self, public_name: &str, args: &Value, mode: Mode, approver: &dyn Approver) -> Result<String, String> {
        let tool = self.tools.iter().find(|t| t.public_name() == public_name).ok_or_else(|| format!("unknown tool: {public_name}"))?;
        let (server, name) = (tool.server.clone(), tool.name.clone());
        // a tool that says it only reads is treated like our own reads; everything else of theirs asks first
        let risk = if tool.read_only { Risk::Read } else { Risk::Change };
        if decide(mode, risk, None) == Decision::Ask && !approver.approve(public_name, &format!("{}\nfrom the MCP server \"{server}\"", mcp::summarize(&name, args))) {
            audit::log(public_name, args, "denied");
            return Err("The person did not approve this (or ZO-AI is not running to ask). Nothing was done.".into());
        }
        let conn = self.conns.iter_mut().find(|c| c.server == server).ok_or("The server is not running.")?;
        let r = conn.call(&name, args);
        audit::log(public_name, args, if r.is_ok() { "ok" } else { "error" });
        r
    }
}

/// Adds-a-server check: stores a pasted access token, starts the server, and, when it asks for a sign-in, opens the person's
/// browser (through `open`) and tries again. Returns the tool names.
pub fn connect(cfg: &ServerConfig, token: Option<&str>, open: &dyn Fn(&str)) -> Result<Vec<String>, String> {
    let token = token.map(str::trim).filter(|t| !t.is_empty());
    if let Some(t) = token {
        crate::ai::keys::set(&token_id(&cfg.name), t)?;
    }
    match test(cfg) {
        Err(e) if token.is_none() && is_url(&cfg.command) && e.starts_with(NEEDS_SIGN_IN) => {
            let opts = crate::mcp_oauth::SignInOptions { client_id: cfg.client_id.clone(), client_secret: cfg.client_secret.clone(), callback_port: cfg.callback_port, scope: cfg.scope.clone() };
            crate::mcp_oauth::sign_in(&cfg.name, &cfg.command, &opts, open)?;
            test(cfg)
        }
        r => r,
    }
}

fn browser_file() -> std::path::PathBuf {
    audit::config_dir().join("browser")
}

/// The command chosen for opening sign-in links (`%u` stands for the address); empty means the system's default browser.
pub fn browser_command() -> String {
    std::fs::read_to_string(browser_file()).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn set_browser_command(cmd: &str) {
    let _ = std::fs::create_dir_all(audit::config_dir());
    let _ = std::fs::write(browser_file(), cmd.trim());
}

/// What to run to open `url`: the chosen browser command, or the system's own opener.
pub fn browser_argv(command: &str, url: &str) -> Vec<String> {
    let words = split_command(command);
    if words.is_empty() {
        return vec!["xdg-open".to_string(), url.to_string()];
    }
    if words.iter().any(|w| w.contains("%u") || w.contains("%U")) {
        words.into_iter().map(|w| w.replace("%u", url).replace("%U", url)).collect()
    } else {
        words.into_iter().chain(std::iter::once(url.to_string())).collect()
    }
}

/// Opens an address in the person's browser (the one chosen in Settings, else the system default).
pub fn open_in_browser(url: &str) {
    let argv = browser_argv(&browser_command(), url);
    let _ = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Starts a server just to see that it works: its tool names, or what went wrong.
pub fn test(cfg: &ServerConfig) -> Result<Vec<String>, String> {
    let mut c = Connection::start(cfg)?;
    Ok(c.tools()?.into_iter().map(|t| t.name).collect())
}

/// A stand-in MCP server written in shell: it answers initialize, tools/list and tools/call.
#[cfg(test)]
pub(crate) fn fake_server() -> (std::path::PathBuf, ServerConfig) {
    // one folder per call: tests run side by side and each removes its own when it is done
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("zo-fake-mcp-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
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
    let cfg = ServerConfig { name: "my fake!".into(), command: format!("sh {}", script.display()), enabled: true, ..Default::default() };
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
        let bad = ServerConfig { name: "nope".into(), command: "/definitely/not/a/program".into(), enabled: true, ..Default::default() };
        let off = ServerConfig { name: "off".into(), command: "/definitely/not/a/program".into(), enabled: false, ..Default::default() };
        let ext = External::start_with(&[bad, off]);
        assert_eq!(ext.problems.len(), 1);
        assert!(ext.problems[0].starts_with("nope:"));
        assert!(ext.tools().is_empty());
        assert!(test(&ServerConfig { name: "x".into(), command: "".into(), enabled: true, ..Default::default() }).is_err());
    }

    /// A stand-in remote MCP server over HTTP: `/mcp` answers (tools/list as an event stream in two pages, the rest as JSON), `/` is not found.
    /// The tool call echoes two request headers back, so a test can see that they were sent.
    fn fake_http_server() -> String {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = Vec::new();
                let mut byte = [0u8; 1];
                while !buf.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    buf.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let header = |name: &str| head.lines().find_map(|l| l.to_lowercase().strip_prefix(&format!("{name}:")).map(|v| v.trim().to_string())).unwrap_or_default();
                let len = header("content-length").parse::<usize>().unwrap_or(0);
                let mut body = vec![0u8; len];
                let _ = stream.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body).to_string();
                let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let id = v["id"].clone();
                let reply = |status: &str, ctype: &str, text: String| format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nMcp-Session-Id: abc\r\nConnection: close\r\n\r\n{text}", text.len());
                let out = if !head.starts_with("POST /mcp ") {
                    reply("404 Not Found", "text/plain", String::new())
                } else if id.is_null() {
                    reply("202 Accepted", "text/plain", String::new())
                } else {
                    match v["method"].as_str().unwrap_or("") {
                        "initialize" => reply("200 OK", "application/json", json!({ "jsonrpc": "2.0", "id": id, "result": { "protocolVersion": "2025-06-18", "capabilities": { "tools": {}, "resources": {} }, "instructions": "Say hi first." } }).to_string()),
                        "tools/list" => {
                            let (tool, next) = if v["params"]["cursor"] == "p2" { ("stop", Value::Null) } else { ("play", json!("p2")) };
                            let hint = tool == "play";
                            reply("200 OK", "text/event-stream", format!("event: message\ndata: {}\n\n", json!({ "jsonrpc": "2.0", "id": id, "result": { "nextCursor": next, "tools": [{ "name": tool, "description": "x", "annotations": { "readOnlyHint": hint }, "inputSchema": { "type": "object", "properties": {} } }] } })))
                        }
                        "resources/list" => reply("200 OK", "application/json", json!({ "jsonrpc": "2.0", "id": id, "result": { "resources": [{ "uri": "doc://a", "name": "A", "mimeType": "text/plain" }] } }).to_string()),
                        _ => reply("200 OK", "application/json", json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": format!("playing|{}|{}", header("x-test"), header("mcp-protocol-version")) }], "isError": false } }).to_string()),
                    }
                };
                let _ = stream.write_all(out.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn a_remote_server_is_reached_by_its_address_even_without_the_mcp_part() {
        let base = fake_http_server();
        for address in [format!("{base}/mcp"), base.clone()] {
            let mut headers = BTreeMap::new();
            headers.insert("X-Test".to_string(), "hello".to_string());
            let cfg = ServerConfig { name: "remote".into(), command: address.clone(), enabled: true, headers, ..Default::default() };
            // two pages of tools, plus our tools for what the server offers besides tools
            assert_eq!(test(&cfg).unwrap(), ["play", "stop", "zo_resources_list", "zo_resources_read"], "{address}");
            let mut ext = External::start_with(&[cfg]);
            assert!(ext.problems.is_empty(), "{:?}", ext.problems);
            assert_eq!(ext.instructions(), [("remote".to_string(), "Say hi first.".to_string())]);
            // a tool that says it only reads runs without asking; one that does not still asks
            assert_eq!(ext.execute("remote__play", &json!({}), Mode::Auto, &Deny).unwrap(), "playing|hello|2025-06-18", "custom headers and the agreed protocol version are sent");
            assert!(ext.execute("remote__stop", &json!({}), Mode::Auto, &Deny).unwrap_err().contains("did not approve"));
            assert!(ext.execute("remote__zo_resources_list", &json!({}), Mode::Auto, &Deny).unwrap().starts_with("doc://a | A | text/plain"));
        }
        let nothing = ServerConfig { name: "gone".into(), command: "http://127.0.0.1:1/mcp".into(), enabled: true, ..Default::default() };
        assert!(test(&nothing).unwrap_err().contains("reach"));
        assert!(is_url(" https://a.b/mcp ") && !is_url("npx server"));
    }

    /// A server whose site address answers 405 (it only has /mcp) and whose /mcp wants a sign-in.
    fn fake_sign_in_server() -> String {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = Vec::new();
                let mut byte = [0u8; 1];
                while !buf.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    buf.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let len = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                let mut body = vec![0u8; len];
                let _ = stream.read_exact(&mut body);
                let out = if head.starts_with("POST /mcp ") {
                    "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer resource_metadata=\"http://x/y\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = stream.write_all(out.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn a_site_address_that_says_405_still_leads_to_the_sign_in_of_its_mcp_address() {
        let base = fake_sign_in_server();
        let cfg = ServerConfig { name: "needs".into(), command: base, enabled: true, ..Default::default() };
        let err = test(&cfg).unwrap_err();
        assert!(err.starts_with(NEEDS_SIGN_IN), "{err}");
    }

    #[test]
    fn a_configuration_pasted_from_another_app_is_understood() {
        let desktop = r#"{"mcpServers":{"files":{"command":"npx","args":["-y","@x/server-files","/home/me/My Docs"],"env":{"TOKEN":"t"}},"spotify":{"url":"https://s.example.com/mcp","headers":{"Authorization":"Bearer z"}}}}"#;
        let got = import_json(desktop).unwrap();
        assert_eq!(got.len(), 2);
        let files = got.iter().find(|c| c.name == "files").unwrap();
        assert_eq!(split_command(&files.command), ["npx", "-y", "@x/server-files", "/home/me/My Docs"]);
        assert_eq!(files.env["TOKEN"], "t");
        let spotify = got.iter().find(|c| c.name == "spotify").unwrap();
        assert_eq!((spotify.command.as_str(), spotify.headers["Authorization"].as_str()), ("https://s.example.com/mcp", "Bearer z"));
        // one server on its own, and VS Code's "servers" key, and a static OAuth client
        assert_eq!(import_json(r#"{"url":"https://a.b/mcp"}"#).unwrap()[0].command, "https://a.b/mcp");
        assert_eq!(import_json(r#"{"servers":{"x":{"command":"run"}}}"#).unwrap()[0].name, "x");
        let o = &import_json(r#"{"mcpServers":{"c":{"url":"https://a.b/mcp","oauth":{"clientId":"id","callbackPort":8080,"scopes":["a","b"]}}}}"#).unwrap()[0];
        assert_eq!((o.client_id.as_deref(), o.callback_port, o.scope.as_deref()), (Some("id"), Some(8080), Some("a b")));
        assert!(import_json("not json").is_err() && import_json(r#"{"mcpServers":{}}"#).is_err());
    }

    #[test]
    fn the_browser_for_sign_in_links_is_the_chosen_command_or_the_system_default() {
        assert_eq!(browser_argv("", "https://a.b/x?y=1"), ["xdg-open", "https://a.b/x?y=1"]);
        assert_eq!(browser_argv("google-chrome-stable --profile-directory=Default %u", "https://a.b"), ["google-chrome-stable", "--profile-directory=Default", "https://a.b"]);
        assert_eq!(browser_argv("firefox -P work", "https://a.b"), ["firefox", "-P", "work", "https://a.b"]);
    }

    #[test]
    fn commands_are_split_like_a_shell_would_and_names_are_cleaned() {
        assert_eq!(split_command(r#"npx -y @scope/server "/path with space" 'a b'"#), ["npx", "-y", "@scope/server", "/path with space", "a b"]);
        assert_eq!(split_command("  one   two  "), ["one", "two"]);
        assert_eq!(clean_name("my fake!"), "my_fake");
        assert_eq!(clean_name("github-server"), "github-server");
    }
}
