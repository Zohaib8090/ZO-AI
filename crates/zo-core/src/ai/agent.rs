//! The loop that makes a model useful: ask, run the tools it calls (through the same permission policy and
//! approval as the MCP server), give it the results, and repeat until it answers in words.

use super::providers::{self, Protocol, Provider};
use std::io::BufRead;
use super::wire::{self, Reply, ToolResult, Turn};
use super::{keys, Selection};
use crate::approval::Approver;
use crate::policy::Mode;
use crate::{mcp, tools};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAX_STEPS: usize = 15;

pub const SYSTEM_PROMPT: &str = "You are ZO-AI, the assistant built into Zohara OS (Arch Linux with KDE Plasma). \
You can look at and change this computer with your tools: files, shell, screenshots, processes, snapshots, Android. \
Look before you change. Say briefly what you are about to do. Risky tools ask the person first; if they refuse, stop and say so. \
Answer in plain, short sentences.";

pub struct Config<'a> {
    pub provider: &'a Provider,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
    pub system: String,
}

impl Config<'static> {
    /// The saved choice, with its key from the keyring.
    pub fn from_selection(sel: &Selection, system: &str) -> Result<Config<'static>, String> {
        let provider = providers::find(&sel.provider).ok_or("No provider is chosen yet.")?;
        if sel.model.is_empty() {
            return Err("No model is chosen yet.".into());
        }
        let key = keys::get(provider.id);
        if key.is_none() && !provider.local {
            return Err(format!("There is no API key for {} yet. Add one in the model settings.", provider.name));
        }
        Ok(Config { provider, base_url: sel.base_url_for(provider), model: sel.model.clone(), key, system: system.into() })
    }
}

pub enum Event {
    /// A new piece of the answer, as it is being written.
    TextDelta(String),
    /// The whole answer of one step, when it is complete.
    Text(String),
    ToolStart { name: String, summary: String },
    ToolDone { name: String, ok: bool, output: String },
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(180)).build()
}

fn http_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body).ok().and_then(|v| v.get("error").map(wire::error_text)).unwrap_or_else(|| body.chars().take(300).collect());
            format!("The provider answered {code}: {msg}")
        }
        other => format!("Could not reach the provider: {other}"),
    }
}

enum Chunk {
    Delta(String),
    Done(Result<Reply, String>),
}

/// Sends the request and reads the answer as it is written, handing each new piece of text on.
fn stream_request(req: wire::Request, protocol: Protocol, tx: &std::sync::mpsc::Sender<Chunk>) -> Result<Reply, String> {
    let mut r = agent().post(&req.url);
    for (k, v) in &req.headers {
        r = r.set(k, v);
    }
    let resp = r.send_json(req.body).map_err(http_error)?;
    let streaming = resp.header("content-type").is_some_and(|c| c.contains("text/event-stream"));
    if !streaming {
        // a server that answers all at once
        let v: Value = resp.into_json().map_err(|e| format!("The answer was not valid JSON: {e}"))?;
        return wire::parse(protocol, &v);
    }
    let mut acc = wire::StreamAcc::default();
    for line in std::io::BufReader::new(resp.into_reader()).lines() {
        let line = line.map_err(|e| format!("The connection broke while the answer was coming: {e}"))?;
        if let Some(d) = acc.feed(protocol, &line)? {
            let _ = tx.send(Chunk::Delta(d));
        }
    }
    Ok(acc.finish())
}

fn ask(cfg: &Config, turns: &[Turn], cancel: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Result<Reply, String> {
    let mut req = wire::build(cfg.provider, &cfg.base_url, cfg.key.as_deref(), &cfg.model, &cfg.system, turns, tools::definitions());
    let protocol = cfg.provider.protocol;
    wire::make_streaming(&mut req, protocol);
    // the request runs on its own thread so Stop works while the model is still thinking
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = stream_request(req, protocol, &tx);
        let _ = tx.send(Chunk::Done(out));
    });
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Chunk::Delta(t)) => on_event(Event::TextDelta(t)),
            Ok(Chunk::Done(r)) => return r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(STOPPED.into());
                }
            }
            Err(_) => return Err("The request ended without an answer.".into()),
        }
    }
}

/// Run one user message to the end. `history` gets every step appended, so it can be saved and continued.
pub const STOPPED: &str = "Stopped.";

pub fn run(cfg: &Config, history: &mut Vec<Turn>, user: &str, mode: Mode, approver: &dyn Approver, cancel: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Result<(), String> {
    history.push(Turn::User(user.to_string()));
    for _ in 0..MAX_STEPS {
        if cancel.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        let reply = ask(cfg, history, cancel, &mut *on_event)?;
        if !reply.text.is_empty() {
            on_event(Event::Text(reply.text.clone()));
        }
        history.push(Turn::Assistant { text: reply.text, calls: reply.calls.clone() });
        if reply.calls.is_empty() {
            return Ok(());
        }
        let mut results = Vec::new();
        for call in reply.calls {
            if cancel.load(Ordering::Relaxed) {
                return Err(STOPPED.into());
            }
            on_event(Event::ToolStart { name: call.name.clone(), summary: mcp::summarize(&call.name, &call.args) });
            let (content, is_error) = match mcp::execute(&call.name, &call.args, mode, approver) {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            on_event(Event::ToolDone { name: call.name.clone(), ok: !is_error, output: content.clone() });
            results.push(ToolResult { id: call.id, name: call.name, content, is_error });
        }
        history.push(Turn::Results(results));
    }
    Err(format!("Stopped after {MAX_STEPS} steps without a final answer."))
}

/// The models a provider offers right now (its own list), or the built-in suggestions if it cannot be asked.
pub fn list_models(p: &Provider, base_url: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    if p.protocol == Protocol::ClaudeCode {
        return Ok(p.models.iter().map(|m| m.to_string()).collect());
    }
    let base = base_url.trim_end_matches('/');
    let mut r = agent().get(&format!("{base}/models"));
    match (p.protocol, key) {
        (Protocol::Anthropic, Some(k)) => r = r.set("x-api-key", k).set("anthropic-version", "2023-06-01"),
        (Protocol::Gemini, Some(k)) => r = r.set("x-goog-api-key", k),
        (_, Some(k)) => r = r.set("authorization", &format!("Bearer {k}")),
        _ => {}
    }
    let v: Value = r.call().map_err(http_error)?.into_json().map_err(|e| e.to_string())?;
    Ok(wire::parse_models(p.protocol, &v))
}

/// Which local model servers are answering on this computer right now.
pub fn detect_local() -> Vec<&'static Provider> {
    providers::all().iter().filter(|p| p.local && p.protocol != Protocol::ClaudeCode).filter(|p| {
        ureq::AgentBuilder::new().timeout(Duration::from_millis(400)).build().get(&format!("{}/models", p.base_url)).call().is_ok()
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::Deny;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A server that answers the given bodies in order and hands back every request body it saw.
    fn mock(answers: Vec<Value>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        serve(answers.into_iter().map(|a| ("application/json".to_string(), a.to_string())).collect())
    }

    fn serve(answers: Vec<(String, String)>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (ctype, body) in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        break (i + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
                seen.push(serde_json::from_slice(&buf[head_end..head_end + len]).unwrap());
                write!(s, "HTTP/1.1 200 OK\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            seen
        });
        (url, h)
    }

    fn cfg(url: String, id: &str) -> Config<'static> {
        Config { provider: providers::find(id).unwrap(), base_url: url, model: "m".into(), key: Some("k".into()), system: "sys".into() }
    }

    #[test]
    fn the_loop_runs_a_tool_and_feeds_the_result_back() {
        let (url, server) = mock(vec![
            json!({"choices":[{"message":{"content":"Checking.","tool_calls":[{"id":"c1","function":{"name":"zohara_shell","arguments":"{\"cmd\":\"echo zo-loop-ok\"}"}}]}}]}),
            json!({"choices":[{"message":{"content":"It printed zo-loop-ok."}}]}),
        ]);
        let mut history = Vec::new();
        let mut events = Vec::new();
        run(&cfg(url, "openai"), &mut history, "say it", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |e| events.push(match e {
            Event::TextDelta(t) => format!("delta:{t}"),
            Event::Text(t) => format!("text:{t}"),
            Event::ToolStart { name, .. } => format!("start:{name}"),
            Event::ToolDone { ok, output, .. } => format!("done:{ok}:{output}"),
        })).unwrap();
        assert_eq!(events, ["text:Checking.", "start:zohara_shell", "done:true:zo-loop-ok", "text:It printed zo-loop-ok."]);
        assert_eq!(history.len(), 4);
        let seen = server.join().unwrap();
        assert_eq!(seen[1]["messages"].as_array().unwrap().last().unwrap()["content"], "zo-loop-ok");
    }

    #[test]
    fn a_refused_tool_is_reported_to_the_model_not_run() {
        let (url, server) = mock(vec![
            json!({"content":[{"type":"tool_use","id":"u1","name":"zohara_fs_rm","input":{"path":"/tmp/zo-never-removed"}}]}),
            json!({"content":[{"type":"text","text":"Understood, I stopped."}]}),
        ]);
        let mut history = Vec::new();
        run(&cfg(url, "anthropic"), &mut history, "delete it", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let seen = server.join().unwrap();
        let result = &seen[1]["messages"].as_array().unwrap().last().unwrap()["content"][0];
        assert_eq!(result["is_error"], true);
        assert!(result["content"].as_str().unwrap().contains("did not approve"));
    }

    #[test]
    fn pressing_stop_ends_the_loop_before_asking_the_model() {
        let err = run(&cfg("http://127.0.0.1:9/v1".into(), "openai"), &mut Vec::new(), "hi", Mode::Auto, &Deny, &AtomicBool::new(true), &mut |_| {}).unwrap_err();
        assert_eq!(err, STOPPED);
    }

    #[test]
    fn a_streamed_answer_arrives_in_pieces_and_its_tool_call_is_assembled() {
        let sse = |lines: &[&str]| ("text/event-stream".to_string(), lines.iter().map(|l| format!("data: {l}\n\n")).collect::<String>());
        let (url, server) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"content":"Checking "}}]}"#,
                r#"{"choices":[{"delta":{"content":"now."}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"zohara_shell","arguments":"{\"cmd\":\"echo "}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"stream-ok\"}"}}]}}]}"#,
                "[DONE]",
            ]),
            sse(&[r#"{"choices":[{"delta":{"content":"It said "}}]}"#, r#"{"choices":[{"delta":{"content":"stream-ok."}}]}"#, "[DONE]"]),
        ]);
        let mut events = Vec::new();
        run(&cfg(url, "openai"), &mut Vec::new(), "go", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |e| events.push(match e {
            Event::TextDelta(t) => format!("delta:{t}"),
            Event::Text(t) => format!("text:{t}"),
            Event::ToolStart { name, .. } => format!("start:{name}"),
            Event::ToolDone { ok, output, .. } => format!("done:{ok}:{output}"),
        })).unwrap();
        assert_eq!(events, ["delta:Checking ", "delta:now.", "text:Checking now.", "start:zohara_shell", "done:true:stream-ok", "delta:It said ", "delta:stream-ok.", "text:It said stream-ok."]);
        let seen = server.join().unwrap();
        assert_eq!(seen[0]["stream"], true);
    }

    #[test]
    fn stop_works_while_the_model_is_still_thinking() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut tmp = [0u8; 8192];
            let _ = s.read(&mut tmp);
            std::thread::sleep(Duration::from_secs(4));
        });
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            c2.store(true, Ordering::Relaxed);
        });
        let t0 = std::time::Instant::now();
        let err = run(&cfg(url, "openai"), &mut Vec::new(), "hi", Mode::Auto, &Deny, &cancel, &mut |_| {}).unwrap_err();
        assert_eq!(err, STOPPED);
        assert!(t0.elapsed() < Duration::from_secs(2), "stop took {:?}", t0.elapsed());
    }

    #[test]
    fn an_http_error_is_a_readable_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut tmp = [0u8; 8192];
            let _ = s.read(&mut tmp);
            let body = r#"{"error":{"message":"invalid api key"}}"#;
            write!(s, "HTTP/1.1 401 Unauthorized\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let err = run(&cfg(url, "openai"), &mut Vec::new(), "hi", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert!(err.contains("401") && err.contains("invalid api key"), "{err}");
    }
}
