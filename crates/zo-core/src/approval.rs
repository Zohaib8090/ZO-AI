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

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join(format!("{}-approve.sock", crate::legacy::NAME))
}

/// The approval sockets of the versions before the renames (Zohara Chat, then ZO-AI); asked when the new one is not there.
pub fn old_socket_paths() -> Vec<PathBuf> {
    crate::legacy::OLD_NAMES.iter().map(|n| runtime_dir().join(format!("{n}-approve.sock"))).collect()
}

/// Asks the running Zohara Agent app, waiting up to two minutes for an answer.
pub struct AppApprover;
impl Approver for AppApprover {
    fn approve(&self, tool: &str, summary: &str) -> bool {
        let Some(mut s) = std::iter::once(socket_path()).chain(old_socket_paths()).find_map(|p| UnixStream::connect(p).ok()) else { return false };
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

#[cfg(test)]
mod rename_tests {
    use super::*;

    /// The approver asks an old app that still runs (the socket of a version before the renames: Zohara Chat or ZO-AI) when the new socket is not there.
    #[test]
    fn the_approver_falls_back_to_each_old_socket() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-ap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let before = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        assert_eq!(socket_path(), dir.join("zohara-agent-approve.sock"));
        assert_eq!(old_socket_paths(), vec![dir.join("zohara-chat-approve.sock"), dir.join("zo-ai-approve.sock")]);
        assert!(!AppApprover.approve("shell", "ls"), "nobody answers: denied");
        for old in old_socket_paths() {
            let l = UnixListener::bind(&old).unwrap();
            let t = std::thread::spawn(move || {
                let (c, _) = l.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&c).read_line(&mut line).unwrap();
                let mut c = c;
                writeln!(c, "allow").unwrap();
                line
            });
            assert!(AppApprover.approve("shell", "ls"), "{}", old.display());
            assert!(t.join().unwrap().contains("shell"));
            std::fs::remove_file(&old).unwrap();
        }
        match before {
            Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
