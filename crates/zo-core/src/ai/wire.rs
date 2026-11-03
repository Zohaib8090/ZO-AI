//! Turning one conversation into each provider's request, and each provider's answer back into one shape.
//! Pure functions, no network, so they are easy to test.

use super::providers::{Protocol, Provider};
use crate::tools::ToolDef;

/// A tool as the model is told about it: ZO-AI's own, or one from an MCP server added in Settings.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// ZO-AI's own tools.
pub fn builtin_specs() -> Vec<ToolSpec> {
    crate::tools::definitions().iter().map(|d: &ToolDef| ToolSpec { name: d.name.into(), description: d.description.into(), schema: d.schema() }).collect()
}
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

/// One step of a conversation, the same for every provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Turn {
    User(String),
    Assistant { text: String, calls: Vec<ToolCall> },
    Results(Vec<ToolResult>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
}

pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

fn has_params(t: &ToolSpec) -> bool {
    t.schema["properties"].as_object().is_some_and(|o| !o.is_empty())
}

pub fn build(p: &Provider, base_url: &str, key: Option<&str>, model: &str, system: &str, turns: &[Turn], tools: &[ToolSpec]) -> Request {
    let base = base_url.trim_end_matches('/');
    match p.protocol {
        Protocol::Anthropic => {
            let messages: Vec<Value> = turns.iter().map(|t| match t {
                Turn::User(s) => json!({ "role": "user", "content": s }),
                Turn::Assistant { text, calls } => {
                    let mut c = Vec::new();
                    if !text.is_empty() {
                        c.push(json!({ "type": "text", "text": text }));
                    }
                    for k in calls {
                        c.push(json!({ "type": "tool_use", "id": k.id, "name": k.name, "input": k.args }));
                    }
                    json!({ "role": "assistant", "content": c })
                }
                Turn::Results(rs) => json!({ "role": "user", "content": rs.iter().map(|r|
                    json!({ "type": "tool_result", "tool_use_id": r.id, "content": r.content, "is_error": r.is_error })).collect::<Vec<_>>() }),
            }).collect();
            let mut body = json!({ "model": model, "max_tokens": 4096, "messages": messages });
            if !system.is_empty() {
                body["system"] = json!(system);
            }
            if !tools.is_empty() {
                body["tools"] = json!(tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.schema.clone() })).collect::<Vec<_>>());
            }
            let mut headers = vec![("anthropic-version".to_string(), "2023-06-01".to_string())];
            if let Some(k) = key {
                headers.push(("x-api-key".into(), k.into()));
            }
            Request { url: format!("{base}/messages"), headers, body }
        }
        Protocol::OpenAi => {
            let mut messages = Vec::new();
            if !system.is_empty() {
                messages.push(json!({ "role": "system", "content": system }));
            }
            for t in turns {
                match t {
                    Turn::User(s) => messages.push(json!({ "role": "user", "content": s })),
                    Turn::Assistant { text, calls } => {
                        let mut m = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) } });
                        if !calls.is_empty() {
                            m["tool_calls"] = json!(calls.iter().map(|k| json!({
                                "id": k.id, "type": "function",
                                "function": { "name": k.name, "arguments": k.args.to_string() } })).collect::<Vec<_>>());
                        }
                        messages.push(m);
                    }
                    Turn::Results(rs) => {
                        for r in rs {
                            messages.push(json!({ "role": "tool", "tool_call_id": r.id, "content": r.content }));
                        }
                    }
                }
            }
            let mut body = json!({ "model": model, "messages": messages });
            if !tools.is_empty() {
                body["tools"] = json!(tools.iter().map(|t| json!({ "type": "function",
                    "function": { "name": t.name, "description": t.description, "parameters": t.schema.clone() } })).collect::<Vec<_>>());
            }
            let headers = key.map(|k| vec![("authorization".to_string(), format!("Bearer {k}"))]).unwrap_or_default();
            Request { url: format!("{base}/chat/completions"), headers, body }
        }
        Protocol::Gemini => {
            let contents: Vec<Value> = turns.iter().map(|t| match t {
                Turn::User(s) => json!({ "role": "user", "parts": [{ "text": s }] }),
                Turn::Assistant { text, calls } => {
                    let mut parts = Vec::new();
                    if !text.is_empty() {
                        parts.push(json!({ "text": text }));
                    }
                    for k in calls {
                        parts.push(json!({ "functionCall": { "name": k.name, "args": k.args } }));
                    }
                    json!({ "role": "model", "parts": parts })
                }
                Turn::Results(rs) => json!({ "role": "user", "parts": rs.iter().map(|r|
                    json!({ "functionResponse": { "name": r.name, "response": { "content": r.content, "is_error": r.is_error } } })).collect::<Vec<_>>() }),
            }).collect();
            let mut body = json!({ "contents": contents });
            if !system.is_empty() {
                body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
            }
            if !tools.is_empty() {
                // Gemini refuses an empty parameter object, so tools without arguments leave it out
                body["tools"] = json!([{ "functionDeclarations": tools.iter().map(|t| {
                    let mut d = json!({ "name": t.name, "description": t.description });
                    if has_params(t) {
                        d["parameters"] = t.schema.clone();
                    }
                    d
                }).collect::<Vec<_>>() }]);
            }
            let headers = key.map(|k| vec![("x-goog-api-key".to_string(), k.to_string())]).unwrap_or_default();
            Request { url: format!("{base}/models/{model}:generateContent"), headers, body }
        }
        Protocol::ClaudeCode => Request { url: String::new(), headers: Vec::new(), body: Value::Null },
    }
}

pub fn parse(protocol: Protocol, v: &Value) -> Result<Reply, String> {
    if let Some(e) = v.get("error") {
        return Err(error_text(e));
    }
    let mut reply = Reply::default();
    match protocol {
        Protocol::ClaudeCode => return Err("Claude Code mode does not use a web protocol.".into()),
        Protocol::Anthropic => {
            for b in v["content"].as_array().ok_or("The answer had no content.")? {
                match b["type"].as_str() {
                    Some("text") => reply.text.push_str(b["text"].as_str().unwrap_or("")),
                    Some("tool_use") => reply.calls.push(ToolCall {
                        id: b["id"].as_str().unwrap_or("").into(),
                        name: b["name"].as_str().unwrap_or("").into(),
                        args: b.get("input").cloned().unwrap_or_else(|| json!({})),
                    }),
                    _ => {}
                }
            }
        }
        Protocol::OpenAi => {
            let m = &v["choices"][0]["message"];
            if m.is_null() {
                return Err("The answer had no message.".into());
            }
            reply.text = m["content"].as_str().unwrap_or("").to_string();
            for (i, c) in m["tool_calls"].as_array().into_iter().flatten().enumerate() {
                let raw = c["function"]["arguments"].as_str().unwrap_or("{}");
                reply.calls.push(ToolCall {
                    id: c["id"].as_str().map(str::to_string).unwrap_or_else(|| format!("call_{i}")),
                    name: c["function"]["name"].as_str().unwrap_or("").into(),
                    args: serde_json::from_str(raw).ok().filter(Value::is_object).unwrap_or_else(|| json!({})),
                });
            }
        }
        Protocol::Gemini => {
            let parts = v["candidates"][0]["content"]["parts"].as_array();
            let Some(parts) = parts else {
                let why = v["candidates"][0]["finishReason"].as_str().or_else(|| v["promptFeedback"]["blockReason"].as_str());
                return Err(format!("Gemini gave no answer{}.", why.map(|w| format!(" ({w})")).unwrap_or_default()));
            };
            for (i, part) in parts.iter().enumerate() {
                if let Some(t) = part["text"].as_str() {
                    reply.text.push_str(t);
                }
                if let Some(fc) = part.get("functionCall") {
                    reply.calls.push(ToolCall {
                        id: format!("call_{i}"),
                        name: fc["name"].as_str().unwrap_or("").into(),
                        args: fc.get("args").cloned().unwrap_or_else(|| json!({})),
                    });
                }
            }
        }
    }
    Ok(reply)
}

pub fn error_text(e: &Value) -> String {
    e["message"].as_str().or_else(|| e.as_str()).map(str::to_string).unwrap_or_else(|| e.to_string())
}

/// Names of the models in a provider's "list models" answer.
pub fn parse_models(protocol: Protocol, v: &Value) -> Vec<String> {
    let mut out: Vec<String> = match protocol {
        Protocol::Gemini => v["models"].as_array().into_iter().flatten()
            .filter(|m| m["supportedGenerationMethods"].as_array().map_or(true, |a| a.iter().any(|x| x == "generateContent")))
            .filter_map(|m| m["name"].as_str()).map(|n| n.trim_start_matches("models/").to_string()).collect(),
        _ => v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).map(str::to_string).collect(),
    };
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::providers::find;

    fn conversation() -> Vec<Turn> {
        vec![
            Turn::User("what is running?".into()),
            Turn::Assistant { text: "Let me look.".into(), calls: vec![ToolCall { id: "t1".into(), name: "zohara_process_list".into(), args: json!({}) }] },
            Turn::Results(vec![ToolResult { id: "t1".into(), name: "zohara_process_list".into(), content: "firefox".into(), is_error: false }]),
        ]
    }

    #[test]
    fn anthropic_request_carries_tool_use_and_result() {
        let r = build(find("anthropic").unwrap(), "https://x/v1/", Some("k"), "m", "sys", &conversation(), &builtin_specs());
        assert_eq!(r.url, "https://x/v1/messages");
        assert!(r.headers.contains(&("x-api-key".into(), "k".into())));
        assert_eq!(r.body["system"], "sys");
        assert_eq!(r.body["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(r.body["messages"][2]["content"][0]["tool_use_id"], "t1");
        assert_eq!(r.body["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn openai_request_uses_tool_role_and_string_arguments() {
        let r = build(find("openai").unwrap(), "https://x/v1", Some("k"), "m", "sys", &conversation(), &builtin_specs());
        assert_eq!(r.url, "https://x/v1/chat/completions");
        assert!(r.headers.contains(&("authorization".into(), "Bearer k".into())));
        assert_eq!(r.body["messages"][0]["role"], "system");
        assert_eq!(r.body["messages"][2]["tool_calls"][0]["function"]["arguments"], "{}");
        assert_eq!(r.body["messages"][3]["role"], "tool");
        assert_eq!(r.body["messages"][3]["tool_call_id"], "t1");
    }

    #[test]
    fn a_local_server_needs_no_key_header() {
        let r = build(find("ollama").unwrap(), "http://127.0.0.1:11434/v1", None, "qwen", "", &[Turn::User("hi".into())], &[]);
        assert!(r.headers.is_empty());
        assert!(r.body.get("tools").is_none());
    }

    #[test]
    fn gemini_request_leaves_out_empty_parameters() {
        let r = build(find("gemini").unwrap(), "https://x/v1beta", Some("k"), "gemini-x", "sys", &conversation(), &builtin_specs());
        assert_eq!(r.url, "https://x/v1beta/models/gemini-x:generateContent");
        assert_eq!(r.body["contents"][1]["role"], "model");
        assert_eq!(r.body["contents"][2]["parts"][0]["functionResponse"]["name"], "zohara_process_list");
        let decls = r.body["tools"][0]["functionDeclarations"].as_array().unwrap();
        let ss = decls.iter().find(|d| d["name"] == "zohara_ss").unwrap();
        assert!(ss.get("parameters").is_none());
        let read = decls.iter().find(|d| d["name"] == "zohara_fs_read").unwrap();
        assert!(read.get("parameters").is_some());
    }

    #[test]
    fn answers_parse_into_text_and_calls() {
        let a = parse(Protocol::Anthropic, &json!({"content":[{"type":"text","text":"ok"},{"type":"tool_use","id":"a","name":"zohara_ss","input":{}}]})).unwrap();
        assert_eq!((a.text.as_str(), a.calls[0].name.as_str()), ("ok", "zohara_ss"));
        let o = parse(Protocol::OpenAi, &json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"b","function":{"name":"zohara_fs_read","arguments":"{\"path\":\"/etc/hostname\"}"}}]}}]})).unwrap();
        assert_eq!(o.calls[0].args["path"], "/etc/hostname");
        let g = parse(Protocol::Gemini, &json!({"candidates":[{"content":{"parts":[{"text":"hi"},{"functionCall":{"name":"zohara_system","args":{}}}]}}]})).unwrap();
        assert_eq!((g.text.as_str(), g.calls.len()), ("hi", 1));
    }

    #[test]
    fn provider_errors_become_messages() {
        assert_eq!(parse(Protocol::OpenAi, &json!({"error":{"message":"bad key"}})).unwrap_err(), "bad key");
        assert!(parse(Protocol::Gemini, &json!({"candidates":[{"finishReason":"SAFETY"}]})).unwrap_err().contains("SAFETY"));
        assert!(parse(Protocol::OpenAi, &json!({})).is_err());
    }

    #[test]
    fn model_lists_parse() {
        assert_eq!(parse_models(Protocol::OpenAi, &json!({"data":[{"id":"b"},{"id":"a"}]})), ["a", "b"]);
        assert_eq!(parse_models(Protocol::Gemini, &json!({"models":[{"name":"models/g1","supportedGenerationMethods":["generateContent"]},{"name":"models/emb","supportedGenerationMethods":["embedContent"]}]})), ["g1"]);
    }
}

// ---------------------------------------------------------------- streaming

/// Turns a request into its streaming form: the provider then sends the answer in small pieces as it writes it.
pub fn make_streaming(req: &mut Request, protocol: Protocol) {
    match protocol {
        Protocol::OpenAi | Protocol::Anthropic => req.body["stream"] = json!(true),
        Protocol::Gemini => req.url = req.url.replace(":generateContent", ":streamGenerateContent?alt=sse"),
        Protocol::ClaudeCode => {}
    }
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

/// Collects a streamed answer line by line. `feed` gives back each new piece of text as it arrives; `finish` gives the whole answer.
#[derive(Default)]
pub struct StreamAcc {
    text: String,
    calls: Vec<PartialCall>,
    /// Anthropic numbers its content blocks; this maps a block number to its place in `calls`.
    blocks: std::collections::HashMap<u64, usize>,
}

impl StreamAcc {
    pub fn feed(&mut self, protocol: Protocol, line: &str) -> Result<Option<String>, String> {
        let Some(data) = line.trim().strip_prefix("data:") else { return Ok(None) };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(None);
        }
        let v: Value = serde_json::from_str(data).map_err(|e| format!("A piece of the answer was not valid JSON: {e}"))?;
        if let Some(e) = v.get("error") {
            return Err(error_text(e));
        }
        let mut delta = String::new();
        match protocol {
            Protocol::OpenAi => {
                let d = &v["choices"][0]["delta"];
                if let Some(t) = d["content"].as_str() {
                    delta.push_str(t);
                }
                for tc in d["tool_calls"].as_array().into_iter().flatten() {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    while self.calls.len() <= idx {
                        self.calls.push(PartialCall::default());
                    }
                    let c = &mut self.calls[idx];
                    if let Some(id) = tc["id"].as_str() {
                        c.id = id.to_string();
                    }
                    if let Some(n) = tc["function"]["name"].as_str().filter(|n| !n.is_empty()) {
                        c.name = n.to_string();
                    }
                    if let Some(a) = tc["function"]["arguments"].as_str() {
                        c.args.push_str(a);
                    }
                }
            }
            Protocol::Anthropic => match v["type"].as_str() {
                Some("content_block_start") if v["content_block"]["type"] == "tool_use" => {
                    self.calls.push(PartialCall {
                        id: v["content_block"]["id"].as_str().unwrap_or("").into(),
                        name: v["content_block"]["name"].as_str().unwrap_or("").into(),
                        args: String::new(),
                    });
                    self.blocks.insert(v["index"].as_u64().unwrap_or(0), self.calls.len() - 1);
                }
                Some("content_block_delta") => {
                    let d = &v["delta"];
                    if let Some(t) = d["text"].as_str() {
                        delta.push_str(t);
                    }
                    if let (Some(p), Some(&at)) = (d["partial_json"].as_str(), self.blocks.get(&v["index"].as_u64().unwrap_or(0))) {
                        self.calls[at].args.push_str(p);
                    }
                }
                _ => {}
            },
            Protocol::Gemini => {
                for part in v["candidates"][0]["content"]["parts"].as_array().into_iter().flatten() {
                    if let Some(t) = part["text"].as_str() {
                        delta.push_str(t);
                    }
                    if let Some(fc) = part.get("functionCall") {
                        self.calls.push(PartialCall {
                            id: String::new(),
                            name: fc["name"].as_str().unwrap_or("").into(),
                            args: fc.get("args").map(|a| a.to_string()).unwrap_or_default(),
                        });
                    }
                }
            }
            Protocol::ClaudeCode => {}
        }
        self.text.push_str(&delta);
        Ok((!delta.is_empty()).then_some(delta))
    }

    pub fn finish(self) -> Reply {
        let calls = self.calls.into_iter().enumerate().map(|(i, c)| ToolCall {
            id: if c.id.is_empty() { format!("call_{i}") } else { c.id },
            name: c.name,
            args: serde_json::from_str(&c.args).ok().filter(Value::is_object).unwrap_or_else(|| json!({})),
        }).collect();
        Reply { text: self.text, calls }
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    fn run(protocol: Protocol, lines: &[&str]) -> (Vec<String>, Reply) {
        let mut acc = StreamAcc::default();
        let mut deltas = Vec::new();
        for l in lines {
            if let Some(d) = acc.feed(protocol, l).unwrap() {
                deltas.push(d);
            }
        }
        (deltas, acc.finish())
    }

    #[test]
    fn openai_text_and_split_tool_arguments() {
        let (d, r) = run(Protocol::OpenAi, &[
            r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#, "",
            r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"zohara_shell","arguments":"{\"cmd\":"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ls\"}"}}]}}]}"#,
            "data: [DONE]",
        ]);
        assert_eq!(d, ["Hel", "lo"]);
        assert_eq!(r.text, "Hello");
        assert_eq!((r.calls[0].id.as_str(), r.calls[0].name.as_str(), r.calls[0].args["cmd"].as_str()), ("c1", "zohara_shell", Some("ls")));
    }

    #[test]
    fn anthropic_blocks() {
        let (d, r) = run(Protocol::Anthropic, &[
            "event: content_block_start",
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Look"}}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"zohara_fs_read"}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"/etc/hostname\"}"}}"#,
        ]);
        assert_eq!(d, ["Look"]);
        assert_eq!((r.calls[0].id.as_str(), r.calls[0].args["path"].as_str()), ("t1", Some("/etc/hostname")));
    }

    #[test]
    fn gemini_chunks() {
        let (d, r) = run(Protocol::Gemini, &[
            r#"data: {"candidates":[{"content":{"parts":[{"text":"Hi "}]}}]}"#,
            r#"data: {"candidates":[{"content":{"parts":[{"text":"there"},{"functionCall":{"name":"zohara_system","args":{}}}]}}]}"#,
        ]);
        assert_eq!(d, ["Hi ", "there"]);
        assert_eq!((r.text.as_str(), r.calls[0].name.as_str(), r.calls[0].id.as_str()), ("Hi there", "zohara_system", "call_0"));
    }

    #[test]
    fn an_error_in_the_stream_is_reported_and_other_lines_are_ignored() {
        let mut acc = StreamAcc::default();
        assert_eq!(acc.feed(Protocol::OpenAi, ": keep-alive").unwrap(), None);
        assert_eq!(acc.feed(Protocol::OpenAi, "event: ping").unwrap(), None);
        assert_eq!(acc.feed(Protocol::OpenAi, r#"data: {"error":{"message":"overloaded"}}"#).unwrap_err(), "overloaded");
    }

    #[test]
    fn gemini_url_becomes_the_streaming_one() {
        let mut r = Request { url: "https://x/v1beta/models/m:generateContent".into(), headers: vec![], body: json!({}) };
        make_streaming(&mut r, Protocol::Gemini);
        assert_eq!(r.url, "https://x/v1beta/models/m:streamGenerateContent?alt=sse");
        let mut o = Request { url: "u".into(), headers: vec![], body: json!({}) };
        make_streaming(&mut o, Protocol::OpenAi);
        assert_eq!(o.body["stream"], true);
    }
}
