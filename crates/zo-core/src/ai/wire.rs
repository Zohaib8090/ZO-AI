//! Turning one conversation into each provider's request, and each provider's answer back into one shape.
//! Pure functions, no network, so they are easy to test.

use super::providers::{Protocol, Provider};
use crate::tools::ToolDef;
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

fn has_params(t: &ToolDef) -> bool {
    !t.params.is_empty()
}

pub fn build(p: &Provider, base_url: &str, key: Option<&str>, model: &str, system: &str, turns: &[Turn], tools: &[ToolDef]) -> Request {
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
                body["tools"] = json!(tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.schema() })).collect::<Vec<_>>());
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
                    "function": { "name": t.name, "description": t.description, "parameters": t.schema() } })).collect::<Vec<_>>());
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
                        d["parameters"] = t.schema();
                    }
                    d
                }).collect::<Vec<_>>() }]);
            }
            let headers = key.map(|k| vec![("x-goog-api-key".to_string(), k.to_string())]).unwrap_or_default();
            Request { url: format!("{base}/models/{model}:generateContent"), headers, body }
        }
    }
}

pub fn parse(protocol: Protocol, v: &Value) -> Result<Reply, String> {
    if let Some(e) = v.get("error") {
        return Err(error_text(e));
    }
    let mut reply = Reply::default();
    match protocol {
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
        let r = build(find("anthropic").unwrap(), "https://x/v1/", Some("k"), "m", "sys", &conversation(), crate::tools::definitions());
        assert_eq!(r.url, "https://x/v1/messages");
        assert!(r.headers.contains(&("x-api-key".into(), "k".into())));
        assert_eq!(r.body["system"], "sys");
        assert_eq!(r.body["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(r.body["messages"][2]["content"][0]["tool_use_id"], "t1");
        assert_eq!(r.body["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn openai_request_uses_tool_role_and_string_arguments() {
        let r = build(find("openai").unwrap(), "https://x/v1", Some("k"), "m", "sys", &conversation(), crate::tools::definitions());
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
        let r = build(find("gemini").unwrap(), "https://x/v1beta", Some("k"), "gemini-x", "sys", &conversation(), crate::tools::definitions());
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
