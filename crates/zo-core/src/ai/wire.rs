//! Turning one conversation into each provider's request, and each provider's answer back into one shape.
//! Pure functions, no network, so they are easy to test.

use super::providers::{Protocol, Provider};
use crate::tools::ToolDef;

/// A tool as the model is told about it: Zohara Chat's own, or one from an MCP server added in Settings.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// Zohara Chat's own tools.
pub fn builtin_specs() -> Vec<ToolSpec> {
    crate::tools::active().into_iter().map(|d: &ToolDef| ToolSpec { name: d.name.into(), description: d.description.into(), schema: d.schema() }).collect()
}
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// A picture on disk that goes to the model along with a message or a tool result. Only the path is kept in a saved chat;
/// the file is read when a request is made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageRef {
    pub path: String,
    pub mime: String,
}

impl ImageRef {
    /// `None` unless the path is an existing picture of a kind the model APIs take.
    pub fn from_path(path: &str) -> Option<ImageRef> {
        let lower = path.to_lowercase();
        let mime = if lower.ends_with(".png") {
            "image/png"
        } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            "image/jpeg"
        } else if lower.ends_with(".webp") {
            "image/webp"
        } else if lower.ends_with(".gif") {
            "image/gif"
        } else {
            return None;
        };
        let meta = std::fs::metadata(path).ok()?;
        (meta.is_file() && meta.len() <= MAX_IMAGE_BYTES).then(|| ImageRef { path: path.to_string(), mime: mime.to_string() })
    }
}

/// A picture that came with a message from the web app: its bytes (base64) are in the request itself. Never saved: after the answer
/// the turn becomes plain text with the picture's name (`agent::forget_inline_images`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InlineImage {
    pub mime: String,
    pub b64: String,
    pub name: String,
}

fn inline_pairs(imgs: &[InlineImage]) -> Vec<(String, String)> {
    // only the four kinds the model APIs take, whatever the turn says
    imgs.iter().filter(|i| matches!(i.mime.as_str(), "image/png" | "image/jpeg" | "image/webp" | "image/gif")).map(|i| (i.mime.clone(), i.b64.clone())).collect()
}

/// The largest picture that is sent (the model APIs refuse about 5 MB and more).
pub const MAX_IMAGE_BYTES: u64 = 4_500_000;

fn base64_std(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// (mime, base64) of the picture at `path`, for an MCP result; `None` if it is not a picture that can be sent.
pub fn encode_path(path: &str) -> Option<(String, String)> {
    encoded(&ImageRef::from_path(path)?)
}

/// (mime, base64) of a picture, read now; `None` if the file is gone or too large.
fn encoded(img: &ImageRef) -> Option<(String, String)> {
    // A saved chat comes from the client (sync), so its picture paths are not trusted: a restricted tenant never has a file read
    // from the server's disk, and for everyone else the kind comes from the file name, never from the saved mime.
    if crate::tenant::is_restricted() {
        return None;
    }
    let checked = ImageRef::from_path(&img.path)?;
    let data = std::fs::read(&checked.path).ok()?;
    (data.len() as u64 <= MAX_IMAGE_BYTES).then(|| (checked.mime, base64_std(&data)))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
    /// Pictures the tool produced (a screenshot): the model is shown them next to the text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageRef>,
}

/// One step of a conversation, the same for every provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Turn {
    User(String),
    /// A message that comes with pictures.
    UserImages(String, Vec<ImageRef>),
    /// A message with pictures that were sent along as data (from the web app): they are used for this question only.
    UserInline(String, Vec<InlineImage>),
    Assistant { text: String, calls: Vec<ToolCall> },
    Results(Vec<ToolResult>),
}

/// What one request used, as the provider reported it. `input` is what was sent fresh; `cached` came from the provider's cache (cheaper
/// and quicker); `written` was put into the cache by this request (Anthropic). Together they are everything that was sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
    pub written: u64,
}

impl Usage {
    pub fn total_input(&self) -> u64 {
        self.input + self.cached + self.written
    }

    fn is_empty(&self) -> bool {
        *self == Usage::default()
    }

    /// Takes what a piece of an answer says about usage (a later piece may only know some of the numbers).
    fn merge(&mut self, other: Usage) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cached = self.cached.max(other.cached);
        self.written = self.written.max(other.written);
    }
}

/// The usage numbers inside an answer (or a piece of one), in each provider's own words.
fn usage_in(protocol: Protocol, usage: &Value) -> Usage {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    match protocol {
        Protocol::Anthropic => Usage { input: n(&usage["input_tokens"]), output: n(&usage["output_tokens"]), cached: n(&usage["cache_read_input_tokens"]), written: n(&usage["cache_creation_input_tokens"]) },
        Protocol::OpenAi => {
            let cached = n(&usage["prompt_tokens_details"]["cached_tokens"]);
            Usage { input: n(&usage["prompt_tokens"]).saturating_sub(cached), output: n(&usage["completion_tokens"]), cached, written: 0 }
        }
        Protocol::Gemini => {
            let cached = n(&usage["cachedContentTokenCount"]);
            Usage { input: n(&usage["promptTokenCount"]).saturating_sub(cached), output: n(&usage["candidatesTokenCount"]), cached, written: 0 }
        }
        Protocol::ClaudeCode => Usage::default(),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
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
                Turn::UserImages(s, imgs) => {
                    let mut c: Vec<Value> = imgs.iter().filter_map(encoded).map(|(m, d)| json!({ "type": "image", "source": { "type": "base64", "media_type": m, "data": d } })).collect();
                    c.push(json!({ "type": "text", "text": s }));
                    json!({ "role": "user", "content": c })
                }
                Turn::UserInline(s, imgs) => {
                    let mut c: Vec<Value> = inline_pairs(imgs).into_iter().map(|(m, d)| json!({ "type": "image", "source": { "type": "base64", "media_type": m, "data": d } })).collect();
                    c.push(json!({ "type": "text", "text": s }));
                    json!({ "role": "user", "content": c })
                }
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
                Turn::Results(rs) => json!({ "role": "user", "content": rs.iter().map(|r| {
                    let pics: Vec<Value> = r.images.iter().filter_map(encoded).map(|(m, d)| json!({ "type": "image", "source": { "type": "base64", "media_type": m, "data": d } })).collect();
                    let content = if pics.is_empty() { json!(r.content) } else { let mut c = vec![json!({ "type": "text", "text": r.content })]; c.extend(pics); json!(c) };
                    json!({ "type": "tool_result", "tool_use_id": r.id, "content": content, "is_error": r.is_error })
                }).collect::<Vec<_>>() }),
            }).collect();
            // top-level cache_control: the provider keeps the start of the conversation (tools, instructions, earlier messages) in its
            // cache, so a long chat is read at a fraction of the price and much faster
            let mut body = json!({ "model": model, "max_tokens": 4096, "messages": messages, "cache_control": { "type": "ephemeral" } });
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
                    Turn::UserImages(s, imgs) => {
                        let mut c = vec![json!({ "type": "text", "text": s })];
                        c.extend(imgs.iter().filter_map(encoded).map(|(m, d)| json!({ "type": "image_url", "image_url": { "url": format!("data:{m};base64,{d}") } })));
                        messages.push(json!({ "role": "user", "content": c }));
                    }
                    Turn::UserInline(s, imgs) => {
                        let mut c = vec![json!({ "type": "text", "text": s })];
                        c.extend(inline_pairs(imgs).into_iter().map(|(m, d)| json!({ "type": "image_url", "image_url": { "url": format!("data:{m};base64,{d}") } })));
                        messages.push(json!({ "role": "user", "content": c }));
                    }
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
                        // a tool message can only hold text, so the pictures a tool made follow as a message of their own
                        let mut pics: Vec<Value> = Vec::new();
                        for r in rs {
                            for (m, d) in r.images.iter().filter_map(encoded) {
                                pics.push(json!({ "type": "text", "text": format!("The picture returned by {}:", r.name) }));
                                pics.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{m};base64,{d}") } }));
                            }
                        }
                        if !pics.is_empty() {
                            messages.push(json!({ "role": "user", "content": pics }));
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
            let contents: Vec<Value> = turns.iter().flat_map(|t| -> Vec<Value> { match t {
                Turn::User(s) => vec![json!({ "role": "user", "parts": [{ "text": s }] })],
                Turn::UserImages(s, imgs) => {
                    let mut parts = vec![json!({ "text": s })];
                    parts.extend(imgs.iter().filter_map(encoded).map(|(m, d)| json!({ "inlineData": { "mimeType": m, "data": d } })));
                    vec![json!({ "role": "user", "parts": parts })]
                }
                Turn::UserInline(s, imgs) => {
                    let mut parts = vec![json!({ "text": s })];
                    parts.extend(inline_pairs(imgs).into_iter().map(|(m, d)| json!({ "inlineData": { "mimeType": m, "data": d } })));
                    vec![json!({ "role": "user", "parts": parts })]
                }
                Turn::Assistant { text, calls } => {
                    let mut parts = Vec::new();
                    if !text.is_empty() {
                        parts.push(json!({ "text": text }));
                    }
                    for k in calls {
                        let mut part = json!({ "functionCall": { "name": k.name, "args": k.args } });
                        // Gemini thinking models need their thoughtSignature handed back (kept after "~" in the id)
                        if let Some((_, sig)) = k.id.split_once('~') {
                            part["thoughtSignature"] = json!(sig);
                        }
                        parts.push(part);
                    }
                    vec![json!({ "role": "model", "parts": parts })]
                }
                Turn::Results(rs) => {
                    let mut out = vec![json!({ "role": "user", "parts": rs.iter().map(|r|
                        json!({ "functionResponse": { "name": r.name, "response": { "content": r.content, "is_error": r.is_error } } })).collect::<Vec<_>>() })];
                    // pictures cannot ride inside a function response, so they follow as a message of their own
                    let mut pics: Vec<Value> = Vec::new();
                    for r in rs {
                        for (m, d) in r.images.iter().filter_map(encoded) {
                            pics.push(json!({ "text": format!("The picture returned by {}:", r.name) }));
                            pics.push(json!({ "inlineData": { "mimeType": m, "data": d } }));
                        }
                    }
                    if !pics.is_empty() {
                        out.push(json!({ "role": "user", "parts": pics }));
                    }
                    out
                }
            } }).collect();
            let mut body = json!({ "contents": contents });
            if !system.is_empty() {
                body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
            }
            if !tools.is_empty() {
                // Gemini refuses an empty parameter object, so tools without arguments leave it out
                body["tools"] = json!([{ "functionDeclarations": tools.iter().map(|t| {
                    let mut d = json!({ "name": t.name, "description": t.description });
                    if has_params(t) {
                        d["parameters"] = gemini_schema(&t.schema);
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

/// Gemini accepts only a small part of JSON Schema (an OpenAPI subset) and answers 400 to the rest
/// ("Unknown name additionalProperties"), so tools from MCP servers are reduced to what it understands.
/// Gemini puts a `thoughtSignature` beside a function call; it is kept in the call id after a "~" so it travels with the call.
fn with_signature(id: String, part: &Value) -> String {
    match part.get("thoughtSignature").or_else(|| part.get("thought_signature")).and_then(Value::as_str) {
        Some(sig) => format!("{id}~{sig}"),
        None => id,
    }
}

pub fn gemini_schema(v: &Value) -> Value {
    const KEEP: &[&str] = &["type", "format", "description", "nullable", "enum", "properties", "required", "items",
        "minItems", "maxItems", "minimum", "maximum", "anyOf", "title"];
    match v {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, val) in m {
                if k == "const" {
                    out.insert("enum".into(), json!([val]));
                    continue;
                }
                if !KEEP.contains(&k.as_str()) {
                    continue;
                }
                match k.as_str() {
                    "properties" => {
                        let props = val.as_object().map(|o| o.iter().map(|(n, p)| (n.clone(), gemini_schema(p))).collect()).unwrap_or_default();
                        out.insert(k.clone(), Value::Object(props));
                    }
                    "type" if val.is_array() => {
                        // ["string","null"] becomes type string + nullable
                        let list = val.as_array().cloned().unwrap_or_default();
                        if let Some(t) = list.iter().find(|t| t.as_str() != Some("null")) {
                            out.insert("type".into(), t.clone());
                        }
                        if list.iter().any(|t| t.as_str() == Some("null")) {
                            out.insert("nullable".into(), json!(true));
                        }
                    }
                    "enum" | "required" | "type" | "format" | "description" | "title" | "nullable" | "minItems" | "maxItems"
                    | "minimum" | "maximum" => {
                        out.insert(k.clone(), val.clone());
                    }
                    _ => {
                        out.insert(k.clone(), gemini_schema(val));
                    }
                }
            }
            // an object or array without the parts Gemini insists on
            if out.get("type").and_then(Value::as_str) == Some("array") && !out.contains_key("items") {
                out.insert("items".into(), json!({ "type": "string" }));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(gemini_schema).collect()),
        other => other.clone(),
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
                        id: with_signature(format!("call_{i}"), part),
                        name: fc["name"].as_str().unwrap_or("").into(),
                        args: fc.get("args").cloned().unwrap_or_else(|| json!({})),
                    });
                }
            }
        }
    }
    let usage = usage_in(protocol, if protocol == Protocol::Gemini { &v["usageMetadata"] } else { &v["usage"] });
    reply.usage = (!usage.is_empty()).then_some(usage);
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
            Turn::Results(vec![ToolResult { id: "t1".into(), name: "zohara_process_list".into(), content: "firefox".into(), is_error: false, images: Vec::new() }]),
        ]
    }

    #[test]
    fn a_restricted_tenant_never_has_a_server_file_read_as_a_picture() {
        let dir = std::env::temp_dir().join(format!("zo-wire-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let secret = dir.join("secret.txt");
        std::fs::write(&secret, "TOPSECRET").unwrap();
        let png = dir.join("a.png");
        std::fs::write(&png, [1u8, 2, 3]).unwrap();
        let evil = ImageRef { path: secret.to_string_lossy().into(), mime: "text/plain".into() };
        let good = ImageRef { path: png.to_string_lossy().into(), mime: "text/plain".into() };
        // not a picture by name: refused for everyone, and the saved mime is not trusted
        assert!(encoded(&evil).is_none());
        assert_eq!(encoded(&good).unwrap().0, "image/png");
        crate::tenant::test_multi(true);
        assert!(encoded(&good).is_none(), "restricted tenants get no file reads");
        let turns = [Turn::UserImages("look".into(), vec![good.clone()])];
        let r = build(find("gemini").unwrap(), "https://x/v1", Some("k"), "m", "", &turns, &[]);
        crate::tenant::test_multi(false);
        assert!(!r.body.to_string().contains("AQID"));
        let _ = std::fs::remove_dir_all(&dir);
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
    fn gemini_thought_signature_comes_back_with_the_call() {
        let g = parse(Protocol::Gemini, &json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"a","args":{}},"thoughtSignature":"AbC+/="}]}}]})).unwrap();
        assert!(g.calls[0].id.ends_with("~AbC+/="));
        let req = build(find("gemini").unwrap(), "https://x/v1beta", None, "m", "", &[Turn::Assistant { text: String::new(), calls: g.calls }], &[]);
        assert_eq!(req.body["contents"][0]["parts"][0]["thoughtSignature"], "AbC+/=");
    }

    #[test]
    fn gemini_schema_drops_what_gemini_does_not_know() {
        let s = json!({"type":"object","additionalProperties":false,"$schema":"x",
            "properties":{"q":{"type":["string","null"],"default":"a","additionalProperties":false},
                          "additionalProperties":{"type":"object","additionalProperties":true,"properties":{}},
                          "l":{"type":"array"}},
            "required":["q"]});
        let g = gemini_schema(&s);
        assert!(g.get("additionalProperties").is_none() && g.get("$schema").is_none());
        assert_eq!(g["properties"]["q"]["type"], "string");
        assert_eq!(g["properties"]["q"]["nullable"], true);
        assert!(g["properties"]["q"].get("default").is_none());
        assert!(g["properties"]["additionalProperties"].get("additionalProperties").is_none());
        assert_eq!(g["properties"]["l"]["items"]["type"], "string");
        assert_eq!(g["required"][0], "q");
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
        Protocol::OpenAi => {
            req.body["stream"] = json!(true);
            // without this an OpenAI-style server does not say how many tokens were used
            req.body["stream_options"] = json!({ "include_usage": true });
        }
        Protocol::Anthropic => req.body["stream"] = json!(true),
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
    usage: Usage,
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
        // token counts come with the first piece (Anthropic), the last (OpenAI) or every piece (Gemini)
        let piece = match protocol {
            Protocol::Anthropic => match v["type"].as_str() {
                Some("message_start") => Some(&v["message"]["usage"]),
                Some("message_delta") => Some(&v["usage"]),
                _ => None,
            },
            Protocol::OpenAi => v.get("usage").filter(|u| !u.is_null()),
            Protocol::Gemini => v.get("usageMetadata"),
            Protocol::ClaudeCode => None,
        };
        if let Some(u) = piece {
            self.usage.merge(usage_in(protocol, u));
        }
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
                            id: with_signature(String::new(), part),
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
            id: if c.id.is_empty() { format!("call_{i}") } else if c.id.starts_with('~') { format!("call_{i}{}", c.id) } else { c.id },
            name: c.name,
            args: serde_json::from_str(&c.args).ok().filter(Value::is_object).unwrap_or_else(|| json!({})),
        }).collect();
        Reply { text: self.text, calls, usage: (!self.usage.is_empty()).then_some(self.usage) }
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

    #[test]
    fn pictures_are_put_where_each_provider_expects_them() {
        let dir = std::env::temp_dir().join(format!("zo-wire-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.png");
        std::fs::write(&f, b"abc").unwrap();
        let img = ImageRef::from_path(&f.to_string_lossy()).unwrap();
        assert_eq!(img.mime, "image/png");
        assert_eq!(base64_std(b"abc"), "YWJj");
        assert_eq!(base64_std(b"ab"), "YWI=");
        assert!(ImageRef::from_path("/no/such/file.png").is_none() && ImageRef::from_path(&dir.join("x.txt").to_string_lossy()).is_none());
        let turns = vec![
            Turn::UserImages("look".into(), vec![img.clone()]),
            Turn::Assistant { text: String::new(), calls: vec![ToolCall { id: "t".into(), name: "zohara_ss".into(), args: json!({}) }] },
            Turn::Results(vec![ToolResult { id: "t".into(), name: "zohara_ss".into(), content: "shot".into(), is_error: false, images: vec![img] }]),
        ];
        let anth = build(crate::ai::providers::find("anthropic").unwrap(), "https://x", Some("k"), "m", "", &turns, &[]).body.to_string();
        assert!(anth.contains("\"type\":\"image\"") && anth.contains("\"media_type\":\"image/png\"") && anth.contains("\"data\":\"YWJj\""));
        let oa = build(crate::ai::providers::find("openai").unwrap(), "https://x", Some("k"), "m", "", &turns, &[]).body;
        assert!(oa["messages"].to_string().contains("data:image/png;base64,YWJj"));
        assert_eq!(oa["messages"].as_array().unwrap().last().unwrap()["role"], "user");
        let gem = build(crate::ai::providers::find("gemini").unwrap(), "https://x", Some("k"), "m", "", &turns, &[]).body;
        let contents = gem["contents"].as_array().unwrap();
        assert!(contents[0]["parts"][1]["inlineData"]["data"] == "YWJj");
        assert_eq!(contents.len(), 4, "the picture of a tool result follows its own message");
        assert!(contents[3]["parts"].to_string().contains("inlineData"));
        // a saved chat from before pictures still loads
        let old: Vec<Turn> = serde_json::from_str(r#"[{"User":"hi"},{"Results":[{"id":"1","name":"n","content":"c","is_error":false}]}]"#).unwrap();
        assert!(matches!(&old[1], Turn::Results(r) if r[0].images.is_empty()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pictures_that_came_in_the_request_are_mapped_for_every_provider_and_are_never_read_from_disk() {
        let img = |n: &str, m: &str| InlineImage { mime: m.into(), b64: "QUJD".into(), name: n.into() };
        let turns = vec![Turn::User("earlier".into()), Turn::Assistant { text: "ok".into(), calls: vec![] }, Turn::UserInline("what is this?".into(), vec![img("a.png", "image/png"), img("b.jpg", "image/jpeg"), img("evil", "text/html")])];
        // Anthropic: base64 sources, images first, then the text; a mime that is not a picture is left out
        let a = build(crate::ai::providers::find("anthropic").unwrap(), "https://x", Some("k"), "m", "", &turns, &[]).body;
        let c = &a["messages"][2]["content"];
        assert_eq!(c.as_array().unwrap().len(), 3);
        assert_eq!(c[0], json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "QUJD"}}));
        assert_eq!(c[1]["source"]["media_type"], "image/jpeg");
        assert_eq!(c[2], json!({"type": "text", "text": "what is this?"}));
        // OpenAI-compatible: the text, then image_url data addresses
        let o = build(crate::ai::providers::find("openai").unwrap(), "https://x", Some("k"), "m", "", &turns, &[]).body;
        let c = &o["messages"][2]["content"];
        assert_eq!(c[0], json!({"type": "text", "text": "what is this?"}));
        assert_eq!(c[1], json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,QUJD"}}));
        assert_eq!(c[2]["image_url"]["url"], "data:image/jpeg;base64,QUJD");
        assert_eq!(c.as_array().unwrap().len(), 3);
        // Gemini: inlineData parts after the text
        let g = build(crate::ai::providers::find("gemini").unwrap(), "https://x", Some("k"), "m", "", &turns, &[]).body;
        let parts = &g["contents"][2]["parts"];
        assert_eq!(parts[0], json!({"text": "what is this?"}));
        assert_eq!(parts[1], json!({"inlineData": {"mimeType": "image/png", "data": "QUJD"}}));
        assert_eq!(parts.as_array().unwrap().len(), 3);
        assert_eq!(g["contents"][2]["role"], "user");
        // a turn that came back from disk with a path-based picture is read for nobody who is restricted
        let dir = std::env::temp_dir().join(format!("zo-wire-restricted-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("secret.png");
        std::fs::write(&f, b"SECRET-BYTES").unwrap();
        let by_path = vec![Turn::UserImages("look".into(), vec![ImageRef::from_path(&f.to_string_lossy()).unwrap()])];
        assert!(build(crate::ai::providers::find("openai").unwrap(), "https://x", Some("k"), "m", "", &by_path, &[]).body.to_string().contains("data:image/png"));
        let t = crate::tenant::testkit::fresh(true);
        crate::tenant::scope(&t, || {
            let body = build(crate::ai::providers::find("openai").unwrap(), "https://x", Some("k"), "m", "", &by_path, &[]).body.to_string();
            assert!(!body.contains("image_url") && !body.contains("U0VDUkVU"), "{body}");
            // but a picture that is in the request itself is fine for them
            let body = build(crate::ai::providers::find("openai").unwrap(), "https://x", Some("k"), "m", "", &turns, &[]).body.to_string();
            assert!(body.contains("data:image/png;base64,QUJD"));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inline_pictures_are_dropped_from_the_conversation_after_the_answer_and_leave_their_names() {
        let img = |n: &str| InlineImage { mime: "image/png".into(), b64: "QUJD".into(), name: n.into() };
        let mut h = vec![Turn::User("a".into()), Turn::UserInline("b".into(), vec![img("x.png"), img("y.png")])];
        crate::ai::agent::forget_inline_images(&mut h);
        assert_eq!(h[1], Turn::User("b\n[image: x.png]\n[image: y.png]".into()));
        assert!(!serde_json::to_string(&h).unwrap().contains("QUJD"));
        // a request that fails because the model takes no pictures drops them too
        let mut h = vec![Turn::UserInline("b".into(), vec![img("x.png")])];
        assert!(crate::ai::agent::has_images(&h));
        crate::ai::agent::strip_images(&mut h);
        assert!(matches!(&h[0], Turn::User(t) if t.contains("1 picture(s) left out")));
        // the turn is saved and loaded like the others
        let t = Turn::UserInline("q".into(), vec![img("z.png")]);
        assert_eq!(serde_json::from_str::<Turn>(&serde_json::to_string(&t).unwrap()).unwrap(), t);
    }

    #[test]
    fn token_usage_is_read_from_each_provider_and_anthropic_is_asked_to_cache() {
        // streamed: Anthropic says the input at the start and the output at the end
        let mut acc = StreamAcc::default();
        acc.feed(Protocol::Anthropic, r#"data: {"type":"message_start","message":{"usage":{"input_tokens":12,"cache_read_input_tokens":900,"cache_creation_input_tokens":40,"output_tokens":1}}}"#).unwrap();
        acc.feed(Protocol::Anthropic, r#"data: {"type":"message_delta","usage":{"output_tokens":55}}"#).unwrap();
        let u = acc.finish().usage.unwrap();
        assert_eq!((u.input, u.cached, u.written, u.output, u.total_input()), (12, 900, 40, 55, 952));
        // OpenAI counts the cached tokens inside the prompt tokens
        let mut acc = StreamAcc::default();
        acc.feed(Protocol::OpenAi, r#"data: {"choices":[{"delta":{"content":"hi"}}],"usage":null}"#).unwrap();
        acc.feed(Protocol::OpenAi, r#"data: {"choices":[],"usage":{"prompt_tokens":1000,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":600}}}"#).unwrap();
        let u = acc.finish().usage.unwrap();
        assert_eq!((u.input, u.cached, u.output, u.total_input()), (400, 600, 7, 1000));
        // not streamed, Gemini
        let r = parse(Protocol::Gemini, &json!({ "candidates": [{ "content": { "parts": [{ "text": "x" }] } }], "usageMetadata": { "promptTokenCount": 50, "candidatesTokenCount": 3, "cachedContentTokenCount": 20 } })).unwrap();
        assert_eq!((r.usage.unwrap().total_input(), r.usage.unwrap().cached), (50, 20));
        assert!(parse(Protocol::OpenAi, &json!({ "choices": [{ "message": { "content": "x" } }] })).unwrap().usage.is_none());
        // Anthropic requests carry the top-level cache setting; streaming OpenAI asks for the usage numbers
        let req = build(crate::ai::providers::find("anthropic").unwrap(), "https://x", Some("k"), "m", "s", &[Turn::User("hi".into())], &[]);
        assert_eq!(req.body["cache_control"]["type"], "ephemeral");
        let mut oa = build(crate::ai::providers::find("openai").unwrap(), "https://x", Some("k"), "m", "s", &[Turn::User("hi".into())], &[]);
        make_streaming(&mut oa, Protocol::OpenAi);
        assert_eq!(oa.body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn which_models_can_look_at_pictures() {
        use crate::ai::providers::{can_see, find};
        assert!(can_see(find("anthropic").unwrap(), "claude-sonnet-5-5"));
        assert!(can_see(find("gemini").unwrap(), "gemini-2.5-flash"));
        assert!(can_see(find("openai").unwrap(), "gpt-4o") && !can_see(find("openai").unwrap(), "gpt-3.5-turbo"));
        assert!(can_see(find("openrouter").unwrap(), "qwen/qwen2.5-vl-72b-instruct") && can_see(find("openrouter").unwrap(), "anthropic/claude-3.5-sonnet"));
        assert!(!can_see(find("openrouter").unwrap(), "cohere/north-mini-code:free") && !can_see(find("deepseek").unwrap(), "deepseek-chat"));
        assert!(!can_see(find("claude-code").unwrap(), "sonnet"));
    }
}
