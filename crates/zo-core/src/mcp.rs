//! The Model Context Protocol over stdio: newline-delimited JSON-RPC 2.0, enough for tools.

use crate::approval::Approver;
use crate::policy::{decide, Decision, Mode, Risk};
use crate::{audit, tools};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// One line that says what is about to happen, for the approval dialog and the log.
pub fn summarize(tool: &str, args: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(o) = args.as_object() {
        for (k, v) in o {
            let s = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            let s = if s.chars().count() > 120 { format!("{}…", s.chars().take(120).collect::<String>()) } else { s };
            parts.push(format!("{k}: {s}"));
        }
    }
    format!("{tool}\n{}", parts.join("\n"))
}

/// Check the policy, ask if needed, run the tool, log it. Used by the app and by the MCP server.
pub fn execute(name: &str, args: &Value, mode: Mode, approver: &dyn Approver) -> Result<String, String> {
    let Some(def) = tools::definitions().iter().find(|d| d.name == name) else {
        return Err(format!("unknown tool: {name}"));
    };
    let shell_cmd = (def.risk == Risk::Shell).then(|| args.get("cmd").and_then(Value::as_str).unwrap_or(""));
    if decide(mode, def.risk, shell_cmd) == Decision::Ask && !approver.approve(name, &summarize(name, args)) {
        audit::log(name, args, "denied");
        return Err("The person did not approve this (or ZO-AI is not running to ask). Nothing was done.".into());
    }
    let result = tools::call(name, args);
    audit::log(name, args, if result.is_ok() { "ok" } else { "error" });
    result
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: &Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Handle one message; `None` for notifications (no answer).
pub fn handle(msg: &Value, mode: Mode, approver: &dyn Approver) -> Option<Value> {
    let id = msg.get("id")?; // no id: a notification
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    Some(match method {
        "initialize" => reply(id, json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-03-26"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "zo-mcp", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Controls Zohara OS: files, shell, screenshots, processes, snapshots, Android. Risky tools ask the person first in the ZO-AI app.",
        })),
        "ping" => reply(id, json!({})),
        "tools/list" => reply(id, json!({ "tools": tools::definitions().iter()
            .map(|d| json!({ "name": d.name, "description": d.description, "inputSchema": d.schema() })).collect::<Vec<_>>() })),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"].get("arguments").cloned().unwrap_or_else(|| json!({}));
            let (text, is_error) = match execute(name, &args, mode, approver) {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            reply(id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        _ => failure(id, -32601, "method not found"),
    })
}

/// Read requests from `input` until it closes. The mode is re-read for each call, so changing it in the app applies at once.
pub fn serve(input: impl BufRead, mut output: impl Write, approver: &dyn Approver) {
    for line in input.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg, Mode::load(), approver),
            Err(_) => Some(failure(&Value::Null, -32700, "parse error")),
        };
        if let Some(a) = answer {
            if writeln!(output, "{a}").and_then(|_| output.flush()).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::Deny;

    fn ask(msg: Value, mode: Mode) -> Value {
        handle(&msg, mode, &Deny).unwrap()
    }

    #[test]
    fn initialize_and_list() {
        let r = ask(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}), Mode::Auto);
        assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(r["result"]["serverInfo"]["name"], "zo-mcp");
        let r = ask(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), Mode::Auto);
        let list = r["result"]["tools"].as_array().unwrap();
        assert!(list.iter().any(|t| t["name"] == "zohara_shell" && t["inputSchema"]["required"][0] == "cmd"));
    }

    #[test]
    fn notifications_get_no_answer() {
        assert!(handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}), Mode::Auto, &Deny).is_none());
    }

    #[test]
    fn a_read_runs_and_a_change_is_refused_without_approval() {
        let r = ask(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"zohara_shell","arguments":{"cmd":"echo hi"}}}), Mode::Auto);
        assert_eq!(r["result"]["content"][0]["text"], "hi");
        assert_eq!(r["result"]["isError"], false);

        let r = ask(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"zohara_fs_rm","arguments":{"path":"/tmp/zo-never"}}}), Mode::Auto);
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("did not approve"));
    }

    #[test]
    fn a_risky_shell_command_asks_in_auto_mode() {
        let r = ask(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"zohara_shell","arguments":{"cmd":"touch /tmp/zo-should-not-exist"}}}), Mode::Auto);
        assert_eq!(r["result"]["isError"], true);
        assert!(!std::path::Path::new("/tmp/zo-should-not-exist").exists());
    }

    #[test]
    fn unknown_method_is_an_error() {
        let r = ask(json!({"jsonrpc":"2.0","id":6,"method":"nope"}), Mode::Auto);
        assert_eq!(r["error"]["code"], -32601);
    }
}
