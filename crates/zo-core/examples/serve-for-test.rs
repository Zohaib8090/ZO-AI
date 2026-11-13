//! A real Zohara Agent service for tests: its socket in $XDG_RUNTIME_DIR, a fake model that asks to run a command and then answers.
//! Used by zohara-link's tests/e2e_zoai_real.py. Never installed.

use serde_json::{json, Value};
use std::io::{Read, Write};

fn main() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    // the model: first it asks to run `echo from-the-laptop` with the shell tool, after the result it says what it saw
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let mut s = conn;
            let mut buf = Vec::new();
            let mut tmp = [0u8; 8192];
            let body = loop {
                let n = s.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break String::new();
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                    let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                    while buf.len() < i + 4 + len {
                        let n = s.read(&mut tmp).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    break String::from_utf8_lossy(&buf[i + 4..]).to_string();
                }
            };
            let v: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let has_result = v["messages"].as_array().map(|m| m.iter().any(|x| x["role"] == "tool")).unwrap_or(false);
            let answer = if has_result {
                let result = v["messages"].as_array().unwrap().iter().rev().find(|x| x["role"] == "tool").map(|x| x["content"].to_string()).unwrap_or_default();
                json!({"choices":[{"message":{"content": format!("The laptop said: {result}")}}]})
            } else {
                json!({"choices":[{"message":{"content":"Let me run that.","tool_calls":[{"id":"c1","type":"function","function":{"name":"zohara_shell","arguments":"{\"cmd\":\"echo from-the-laptop && touch /tmp/zo-e2e-touched\"}"}}]}}]})
            };
            let b = answer.to_string();
            let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}", b.len());
        }
    });
    let mut sel = zo_core::ai::Selection { provider: "ollama".into(), model: "m".into(), ..Default::default() };
    sel.base_urls.insert("ollama".into(), url);
    sel.save();
    zo_core::policy::Mode::Manual.save();
    zo_core::service::start().expect("service");
    println!("READY {}", zo_core::service::socket_path().display());
    std::io::stdout().flush().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(90));
}
