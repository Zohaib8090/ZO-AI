//! A pretend OpenAI-compatible model server for testing the chat without a real key or network.
//! First it asks to run a shell command, then it repeats the result. Usage: mock-model [port]

use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "8011".into());
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).expect("bind");
    eprintln!("mock model on port {port}");
    for stream in listener.incoming().flatten() {
        let mut s = stream;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        let (head_end, len) = loop {
            let n = s.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break (0, 0);
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0usize);
                break (i + 4, len);
            }
        };
        while head_end > 0 && buf.len() < head_end + len {
            let n = s.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        let request = String::from_utf8_lossy(&buf).to_string();
        let first = request.lines().next().unwrap_or("").to_string();
        let body = request.get(head_end..).unwrap_or("");
        let answer = if first.contains("/models") {
            serde_json::json!({ "data": [{ "id": "mock-small" }, { "id": "mock-large" }] })
        } else {
            let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let last = v["messages"].as_array().and_then(|m| m.last()).cloned().unwrap_or_default();
            if last["role"] == "tool" {
                serde_json::json!({ "choices": [{ "message": { "content": format!("The command printed: {}", last["content"].as_str().unwrap_or("")) } }] })
            } else {
                serde_json::json!({ "choices": [{ "message": { "content": "I will run a command to check.", "tool_calls": [{
                    "id": "call_1", "type": "function",
                    "function": { "name": "zohara_shell", "arguments": "{\"cmd\":\"echo hello-from-zoai\"}" } }] } }] })
            }
        };
        let out = answer.to_string();
        let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}", out.len());
    }
}
