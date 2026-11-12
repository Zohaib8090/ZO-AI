//! Talking to the service: over the local socket or over HTTPS to a zo-server. Both give the same two things: a one-shot
//! `op` and a running `send` stream whose events arrive on a channel and which can be answered (approve, stop).

use crate::proto::{parse_event, Event};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum Target {
    Local(PathBuf),
    Remote { url: String, token: Option<String> },
}

/// A running answer. Events come on `rx`; an `Err` means the connection broke.
pub struct Stream {
    pub rx: Receiver<Result<Event, String>>,
    reply: Box<dyn Fn(&Value) -> Result<(), String> + Send>,
    _guard: Option<Box<dyn Send>>,
}

impl Stream {
    #[cfg(test)]
    pub fn new(rx: Receiver<Result<Event, String>>, reply: Box<dyn Fn(&Value) -> Result<(), String> + Send>) -> Stream {
        Stream { rx, reply, _guard: None }
    }
    pub fn reply(&self, v: &Value) -> Result<(), String> {
        (self.reply)(v)
    }
}

/// Closing the socket tells the service to stop the answer, so it is closed when the stream is dropped.
struct Shut(UnixStream);
impl Drop for Shut {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

pub fn local_hint(path: &std::path::Path) -> String {
    format!(
        "ZO-AI is not running: nothing listens on {}.\nStart the ZO-AI app (it serves this socket), or talk to a server instead:\n  zo --remote https://your-server --token-file ~/.config/zo-ai/token \"question\"\n  (or set ZO_REMOTE_URL, and ZO_ACCESS_TOKEN for the token)",
        path.display()
    )
}

fn check(v: Value) -> Result<Value, String> {
    if v.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(v.get("error").and_then(Value::as_str).unwrap_or("the service refused").to_string());
    }
    Ok(v)
}

fn remote_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(401, _) => "The server refused the token (401). Check the file or variable that holds it (--token-file, or $ZO_ACCESS_TOKEN).".into(),
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body).ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
            format!("The server answered {code}{}", msg.map(|m| format!(": {m}")).unwrap_or_default())
        }
        ureq::Error::Transport(t) => format!("Cannot reach the server: {t}"),
    }
}

impl Target {
    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).build()
    }

    fn post(url: &str, path: &str, token: &Option<String>, body: &Value) -> Result<ureq::Response, String> {
        let mut r = Self::agent().post(&format!("{}{path}", url.trim_end_matches('/'))).set("Content-Type", "application/json");
        if let Some(t) = token {
            r = r.set("Authorization", &format!("Bearer {t}"));
        }
        r.send_string(&body.to_string()).map_err(remote_error)
    }

    /// One question, one answer.
    pub fn op(&self, req: &Value) -> Result<Value, String> {
        match self {
            Target::Local(path) => {
                let mut conn = UnixStream::connect(path).map_err(|_| local_hint(path))?;
                conn.set_read_timeout(Some(Duration::from_secs(180))).ok();
                conn.write_all(format!("{req}\n").as_bytes()).map_err(|e| e.to_string())?;
                let mut line = String::new();
                BufReader::new(conn).read_line(&mut line).map_err(|e| e.to_string())?;
                check(serde_json::from_str(&line).map_err(|_| "ZO-AI gave no answer.".to_string())?)
            }
            Target::Remote { url, token } => {
                let resp = Self::post(url, "/api/op", token, req)?;
                check(resp.into_json::<Value>().map_err(|_| "The server gave no JSON answer.".to_string())?)
            }
        }
    }

    /// Sends a message; the answer streams.
    pub fn send(&self, req: &Value) -> Result<Stream, String> {
        let (tx, rx) = mpsc::channel();
        match self {
            Target::Local(path) => {
                let conn = UnixStream::connect(path).map_err(|_| local_hint(path))?;
                let mut w = conn.try_clone().map_err(|e| e.to_string())?;
                w.write_all(format!("{req}\n").as_bytes()).map_err(|e| e.to_string())?;
                let shut = Shut(conn.try_clone().map_err(|e| e.to_string())?);
                std::thread::spawn(move || pump(BufReader::new(conn), tx, None));
                let w = Mutex::new(w);
                let reply: Box<dyn Fn(&Value) -> Result<(), String> + Send> = Box::new(move |v| w.lock().unwrap().write_all(format!("{v}\n").as_bytes()).map_err(|e| e.to_string()));
                Ok(Stream { rx, reply, _guard: Some(Box::new(shut)) })
            }
            Target::Remote { url, token } => {
                let resp = Self::post(url, "/api/send", token, req)?;
                let id = Arc::new(AtomicU64::new(0));
                let id2 = id.clone();
                std::thread::spawn(move || pump(BufReader::new(resp.into_reader()), tx, Some(id2)));
                let (url, token) = (url.clone(), token.clone());
                let reply: Box<dyn Fn(&Value) -> Result<(), String> + Send> = Box::new(move |v| {
                    let n = id.load(Ordering::SeqCst);
                    if n == 0 {
                        return Err("the answer has no number yet".into());
                    }
                    let mut v = v.clone();
                    v["stream"] = n.into();
                    Self::post(&url, "/api/reply", &token, &v).map(|_| ())
                });
                Ok(Stream { rx, reply, _guard: None })
            }
        }
    }
}

/// Reads lines until the stream ends, and passes the events on.
fn pump<R: BufRead>(mut r: R, tx: mpsc::Sender<Result<Event, String>>, stream_id: Option<Arc<AtomicU64>>) {
    let mut line = String::new();
    let mut done = false;
    loop {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if let Some(e) = parse_event(&line) {
                    if let (Event::Stream(n), Some(id)) = (&e, &stream_id) {
                        id.store(*n, Ordering::SeqCst);
                    }
                    done = e == Event::Done;
                    if tx.send(Ok(e)).is_err() || done {
                        return;
                    }
                }
            }
        }
    }
    if !done {
        let _ = tx.send(Err("The connection closed before the answer was finished.".into()));
    }
}
