//! The protocol, without any I/O: parse the event lines the service sends, build the requests, and draw events on a terminal.

use serde_json::{json, Value};
use std::io::Write;

/// One line of a `send` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Remote only: the number of this answer, for /api/reply.
    Stream(u64),
    Chat { id: String, title: String },
    Delta(String),
    Text(String),
    ToolStart { name: String, summary: String },
    ToolDone { name: String, ok: bool, output: String },
    Approval { id: u64, tool: String, summary: String },
    Usage { input: u64, output: u64 },
    Error(String),
    Stopped,
    Done,
}

fn st(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Reads one line. `None` for a blank line, a line that is not JSON, or an event this client does not know (newer service).
pub fn parse_event(line: &str) -> Option<Event> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    Some(match v.get("event")?.as_str()? {
        "stream" => Event::Stream(n("id")),
        "chat" => Event::Chat { id: st(&v, "id"), title: st(&v, "title") },
        "delta" => Event::Delta(st(&v, "text")),
        "text" => Event::Text(st(&v, "text")),
        "tool_start" => Event::ToolStart { name: st(&v, "name"), summary: st(&v, "summary") },
        "tool_done" => Event::ToolDone { name: st(&v, "name"), ok: v.get("ok").and_then(Value::as_bool).unwrap_or(true), output: st(&v, "output") },
        "approval" => Event::Approval { id: n("id"), tool: st(&v, "tool"), summary: st(&v, "summary") },
        "usage" => Event::Usage { input: n("input"), output: n("output") },
        "error" => Event::Error(st(&v, "error")),
        "stopped" => Event::Stopped,
        "done" => Event::Done,
        _ => return None,
    })
}

/// The request line that sends a message.
pub fn send_request(text: &str, chat: Option<&str>, mode: Option<&str>) -> Value {
    let mut v = json!({"op": "send", "text": text});
    if let Some(c) = chat.filter(|c| !c.is_empty()) {
        v["chat"] = json!(c);
    }
    if let Some(m) = mode.filter(|m| !m.is_empty()) {
        v["mode"] = json!(m);
    }
    v
}

pub fn approve_reply(id: u64, yes: bool) -> Value {
    json!({"op": "approve", "id": id, "yes": yes})
}

pub fn stop_reply() -> Value {
    json!({"op": "stop"})
}

/// "5 min ago", for the chat list.
pub fn ago(now_ms: u64, then_ms: u64) -> String {
    let s = now_ms.saturating_sub(then_ms) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86400),
    }
}

/// Cuts a text to one line of at most `max` characters.
pub fn one_line(s: &str, max: usize) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= max {
        joined
    } else {
        format!("{}…", joined.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

/// Draws events: the answer on `out`, everything else (tool steps, errors) on `err`.
pub struct Renderer<O: Write, E: Write> {
    pub out: O,
    pub err: E,
    pub color_err: bool,
    /// Text of the current message was already printed as deltas.
    streamed: bool,
    at_line_start: bool,
    pub failed: bool,
    pub chat: Option<String>,
    pub tokens: (u64, u64),
}

impl<O: Write, E: Write> Renderer<O, E> {
    pub fn new(out: O, err: E, color_err: bool) -> Self {
        Renderer { out, err, color_err, streamed: false, at_line_start: true, failed: false, chat: None, tokens: (0, 0) }
    }

    fn paint(on: bool, code: &str, s: &str) -> String {
        if on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }

    pub fn dim_err(&mut self, s: &str) {
        self.break_line();
        let _ = writeln!(self.err, "{}", Self::paint(self.color_err, "2", s));
        let _ = self.err.flush();
    }

    pub fn error(&mut self, s: &str) {
        self.break_line();
        let _ = writeln!(self.err, "{}", Self::paint(self.color_err, "1;31", &format!("error: {s}")));
        let _ = self.err.flush();
    }

    pub fn print_out(&mut self, s: &str) {
        let _ = self.out.write_all(s.as_bytes());
        let _ = self.out.flush();
        if let Some(c) = s.chars().last() {
            self.at_line_start = c == '\n';
        }
    }

    /// If the answer stopped in the middle of a line, end it before something else is written.
    fn break_line(&mut self) {
        if !self.at_line_start {
            let _ = self.out.write_all(b"\n");
            let _ = self.out.flush();
            self.at_line_start = true;
        }
    }

    pub fn event(&mut self, e: &Event) {
        match e {
            Event::Chat { id, .. } => self.chat = Some(id.clone()),
            Event::Delta(t) => {
                self.streamed = true;
                self.print_out(t);
            }
            Event::Text(t) => {
                if !self.streamed {
                    self.print_out(t);
                }
                self.streamed = false;
                self.break_line();
            }
            Event::ToolStart { name, summary } => {
                self.streamed = false;
                let line = format!("▸ {name}  {}", one_line(summary.strip_prefix(name.as_str()).unwrap_or(summary), 160));
                self.dim_err(line.trim_end());
            }
            Event::ToolDone { ok, output, .. } => {
                let line = format!("  {} {}", if *ok { "✓" } else { "✗" }, one_line(output, 160));
                if *ok { self.dim_err(line.trim_end()) } else { self.break_line(); let _ = writeln!(self.err, "{}", Self::paint(self.color_err, "31", line.trim_end())); }
            }
            Event::Approval { tool, summary, .. } => {
                self.break_line();
                let _ = writeln!(self.err, "{}", Self::paint(self.color_err, "1", &format!("Allow {tool}?")));
                let body = summary.strip_prefix(tool.as_str()).unwrap_or(summary).trim_start_matches('\n');
                for l in body.lines().take(12) {
                    let _ = writeln!(self.err, "  {l}");
                }
            }
            Event::Usage { input, output } => self.tokens = (self.tokens.0 + input, self.tokens.1 + output),
            Event::Error(m) => {
                self.failed = true;
                self.error(m);
            }
            Event::Stopped => self.dim_err("(stopped)"),
            Event::Done => self.break_line(),
            Event::Stream(_) => {}
        }
    }
}

/// `y`, `yes`, `j`... anything else is no.
pub fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(color: bool) -> Renderer<Vec<u8>, Vec<u8>> {
        Renderer::new(Vec::new(), Vec::new(), color)
    }
    fn s(v: &[u8]) -> String {
        String::from_utf8_lossy(v).to_string()
    }

    #[test]
    fn parses_every_event() {
        assert_eq!(parse_event(r#"{"event":"stream","id":7}"#), Some(Event::Stream(7)));
        assert_eq!(parse_event(r#"{"event":"chat","id":"c1","title":"hi"}"#), Some(Event::Chat { id: "c1".into(), title: "hi".into() }));
        assert_eq!(parse_event(r#"{"event":"delta","text":"a"}"#), Some(Event::Delta("a".into())));
        assert_eq!(parse_event(r#"{"event":"approval","id":3,"tool":"shell","summary":"ls"}"#), Some(Event::Approval { id: 3, tool: "shell".into(), summary: "ls".into() }));
        assert_eq!(parse_event(r#"{"event":"tool_done","name":"x","ok":false,"output":"no"}"#), Some(Event::ToolDone { name: "x".into(), ok: false, output: "no".into() }));
        assert_eq!(parse_event(r#"{"event":"usage","input":5,"output":2}"#), Some(Event::Usage { input: 5, output: 2 }));
        assert_eq!(parse_event(r#"{"event":"done"}"#), Some(Event::Done));
        assert_eq!(parse_event(r#"{"event":"stopped"}"#), Some(Event::Stopped));
        assert_eq!(parse_event(r#"{"event":"error","error":"bad"}"#), Some(Event::Error("bad".into())));
        assert_eq!(parse_event(r#"{"event":"from_the_future"}"#), None);
        assert_eq!(parse_event("not json"), None);
        assert_eq!(parse_event(""), None);
    }

    #[test]
    fn builds_requests() {
        assert_eq!(send_request("hi", None, None), json!({"op": "send", "text": "hi"}));
        assert_eq!(send_request("hi", Some("c1"), Some("Auto")), json!({"op": "send", "text": "hi", "chat": "c1", "mode": "Auto"}));
        assert_eq!(send_request("hi", Some(""), None), json!({"op": "send", "text": "hi"}));
        assert_eq!(approve_reply(4, true), json!({"op": "approve", "id": 4, "yes": true}));
        assert_eq!(stop_reply(), json!({"op": "stop"}));
    }

    #[test]
    fn deltas_are_not_printed_twice_by_the_final_text() {
        let mut x = r(false);
        x.event(&Event::Delta("Hel".into()));
        x.event(&Event::Delta("lo".into()));
        x.event(&Event::Text("Hello".into()));
        x.event(&Event::Done);
        assert_eq!(s(&x.out), "Hello\n");
        // without deltas the text is printed
        let mut y = r(false);
        y.event(&Event::Text("Whole".into()));
        assert_eq!(s(&y.out), "Whole\n");
    }

    #[test]
    fn tool_steps_go_to_stderr_and_end_the_line_on_stdout() {
        let mut x = r(false);
        x.event(&Event::Delta("Let me look".into()));
        x.event(&Event::ToolStart { name: "shell".into(), summary: "Run a command\n  ls -la".into() });
        x.event(&Event::ToolDone { name: "shell".into(), ok: true, output: "a\nb".into() });
        assert_eq!(s(&x.out), "Let me look\n");
        assert_eq!(s(&x.err), "▸ shell  Run a command ls -la\n  ✓ a b\n");
        assert!(!x.failed);
    }

    #[test]
    fn errors_mark_failure_and_colors_follow_the_flag() {
        let mut x = r(false);
        x.event(&Event::Error("No model".into()));
        assert!(x.failed);
        assert_eq!(s(&x.err), "error: No model\n");
        let mut y = r(true);
        y.event(&Event::Stopped);
        assert_eq!(s(&y.err), "\x1b[2m(stopped)\x1b[0m\n");
    }

    #[test]
    fn small_helpers() {
        assert!(is_yes(" Y\n") && is_yes("yes") && !is_yes("") && !is_yes("no") && !is_yes("yep"));
        assert_eq!(ago(100_000, 70_000), "just now");
        assert_eq!(ago(10 * 60_000, 0), "10 min ago");
        assert_eq!(ago(3 * 3_600_000, 0), "3 h ago");
        assert_eq!(ago(2 * 86_400_000, 0), "2 d ago");
        assert_eq!(ago(0, 5), "just now");
        assert_eq!(one_line("a  b\nc", 10), "a b c");
        assert_eq!(one_line("abcdefghij", 5), "abcd…");
    }
}
