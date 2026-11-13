//! Asking the person. The app listens on a Unix socket; the MCP server (a separate process) connects to it
//! when a tool needs approval. If the app is not running there is nobody to ask, so the answer is no.
//! Also: the question tool (`zohara_ask_user`) lets the AI ask questions with ready-made options.

use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

// ── Approvals ──

pub trait Approver {
    fn approve(&self, tool: &str, summary: &str) -> bool;
}

/// Never approves: for when there is no one to ask.
pub struct Deny;
impl Approver for Deny {
    fn approve(&self, _: &str, _: &str) -> bool {
        false
    }
}

// ── Questions (zohara_ask_user) ──

/// One option inside a question card.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// A single question with its header chip and options.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
}

/// The person's answer to one question.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuestionAnswer {
    pub selected: Vec<String>,
    pub other: Option<String>,
}

/// What came of asking.
#[derive(Debug, Clone, PartialEq)]
pub enum QuestionResult {
    /// One answer per question, same order, already checked against the questions (see `question::validate_answers`).
    Answers(Vec<QuestionAnswer>),
    /// The person pressed Skip.
    Declined,
    /// Nobody answered: timeout, Stop, the client left, or this client cannot show questions.
    Unanswered,
}

/// Ask the person questions with ready-made options.
pub trait Questioner {
    fn ask_questions(&self, questions: &[Question]) -> QuestionResult;
    /// False when there is nobody to ask: the tool is then not offered to the model at all.
    fn can_ask(&self) -> bool {
        true
    }
}

/// Never gets an answer: for when there is no one to ask (CLI, sub-agents, scheduled tasks, tests).
pub struct NoQuestions;
impl Questioner for NoQuestions {
    fn ask_questions(&self, _: &[Question]) -> QuestionResult {
        QuestionResult::Unanswered
    }
    fn can_ask(&self) -> bool {
        false
    }
}

pub fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("zo-ai-approve.sock")
}

/// Asks the running ZO-AI app, waiting up to two minutes for an answer.
pub struct AppApprover;
impl Approver for AppApprover {
    fn approve(&self, tool: &str, summary: &str) -> bool {
        let Ok(mut s) = UnixStream::connect(socket_path()) else { return false };
        let _ = s.set_read_timeout(Some(Duration::from_secs(120)));
        if writeln!(s, "{}", json!({ "tool": tool, "summary": summary })).is_err() {
            return false;
        }
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).is_ok() && line.trim() == "allow"
    }
}

/// Listen for questions and answer each with `ask` (called on a worker thread, may block on a dialog).
pub fn serve(ask: impl Fn(String, String) -> bool + Send + Sync + 'static) -> std::io::Result<()> {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    let ask = std::sync::Arc::new(ask);
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let ask = ask.clone();
            std::thread::spawn(move || {
                let mut line = String::new();
                if BufReader::new(&conn).read_line(&mut line).is_err() {
                    return;
                }
                let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                let tool = v["tool"].as_str().unwrap_or("").to_string();
                let summary = v["summary"].as_str().unwrap_or("").to_string();
                let ok = ask(tool, summary);
                let mut conn = conn;
                let _ = writeln!(conn, "{}", if ok { "allow" } else { "deny" });
            });
        }
    });
    Ok(())
}
