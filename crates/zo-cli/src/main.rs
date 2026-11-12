//! `zo`: ZO-AI in the terminal. Talks to the local ZO-AI service (the app's Unix socket) or to a zo-server with --remote.

mod net;
mod proto;

use net::{Stream, Target};
use proto::{approve_reply, ago, is_yes, one_line, send_request, stop_reply, Event, Renderer};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{IsTerminal, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

const USAGE: &str = "zo: ZO-AI in the terminal

Usage:
  zo \"question\"              ask once; the answer streams to stdout, tool steps go to stderr
  echo \"question\" | zo       the same, with the question on stdin
  zo chat [chat-id]          talk interactively (continues a saved chat when given its id)
  zo ask <text>              ask, even when the text starts with a command word
  zo chats                   list saved chats
  zo show <chat-id>          print a saved chat
  zo memory                  list what ZO-AI remembers
  zo remember \"text\"         remember something
  zo forget <note-id>        forget a note
  zo status                  model, permission mode, version
  zo models [provider]       providers, and the models of one
  zo use <provider> [model]  choose provider and model
  zo mode [Manual|Auto|Bypass]  show or set the permission mode

Options:
  -c, --chat ID          continue this chat (one-shot questions)
      --mode MODE        permission mode for this question: Manual, Auto or Bypass
      --remote URL       talk to a zo-server instead of the local app (or set ZO_REMOTE_URL)
      --stdin            add piped input to the question on the command line
      --local            ignore ZO_REMOTE_URL and use the local app
      --token-env NAME   environment variable that holds the device token (scope: full), default ZO_ACCESS_TOKEN
      --token-file PATH  file that holds the device token (scope: full)
      --no-color         no colours (also: NO_COLOR is set, or the output is not a terminal)
  -h, --help             this text
  -V, --version

The device token (scope: full, starts with zd_) is made in the web app: Settings > Account > Create device token.
It is never taken as an argument value: it would show in the process list.
Inside `zo chat`: /new /chats /open N /model [name] /mode [name] /memory /stop /help /quit.
Ctrl-C stops the running answer; a second Ctrl-C quits. Ctrl-D quits. Exit codes: 0 ok, 1 error, 2 usage.";

#[derive(Debug, Default, PartialEq)]
struct Opts {
    remote: Option<String>,
    local: bool,
    token_env: Option<String>,
    token_file: Option<String>,
    chat: Option<String>,
    mode: Option<String>,
    no_color: bool,
    stdin: bool,
    help: bool,
    version: bool,
    rest: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut i = 0;
    let mut flags_done = false;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        if flags_done || !a.starts_with('-') || a == "-" {
            o.rest.push(a.to_string());
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a, None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            match inline.clone() {
                Some(v) => Ok(v),
                None if i < args.len() => {
                    i += 1;
                    Ok(args[i - 1].clone())
                }
                None => Err(format!("{name} needs {what}")),
            }
        };
        match name {
            "--" => flags_done = true,
            "-h" | "--help" => o.help = true,
            "-V" | "--version" => o.version = true,
            "--no-color" => o.no_color = true,
            "--local" => o.local = true,
            "--stdin" => o.stdin = true,
            "--remote" => o.remote = Some(value("an address")?),
            "--token-env" => o.token_env = Some(value("a variable name")?),
            "--token-file" => o.token_file = Some(value("a file name")?),
            "-c" | "--chat" => o.chat = Some(value("a chat id")?),
            "--mode" => o.mode = Some(value("a mode")?),
            "--token" | "--access-token" | "--password" => {
                return Err("Refusing a device token as an argument: it would show in the process list.\nPut it in a file and use --token-file PATH, or in an environment variable and use --token-env NAME (default ZO_ACCESS_TOKEN). Make a device token (scope: full) in the web app: Settings > Account.".into());
            }
            _ => return Err(format!("unknown option {a} (try --help)")),
        }
    }
    Ok(o)
}

/// The device token from the file, else from the environment variable. `None` when neither is set (a local test server without login needs none).
/// A token that is set must look like a device token (`zd_...`).
fn read_token(file: Option<&str>, env_name: &str, getenv: &dyn Fn(&str) -> Option<String>) -> Result<Option<String>, String> {
    if let Some(p) = file {
        let t = std::fs::read_to_string(p).map_err(|e| format!("cannot read the token file {p}: {e}"))?;
        let t = t.trim().to_string();
        if t.is_empty() {
            return Err(format!("the token file {p} is empty"));
        }
        return zo_core::sync::check_device_token(&t).map(Some).map_err(|e| format!("{e} (token file {p})"));
    }
    match getenv(env_name).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        Some(t) => zo_core::sync::check_device_token(&t).map(Some).map_err(|e| format!("{e} (variable ${env_name})")),
        None => Ok(None),
    }
}

fn canonical_mode(s: &str) -> Option<&'static str> {
    match s.trim().to_lowercase().as_str() {
        "manual" => Some("Manual"),
        "auto" => Some("Auto"),
        "bypass" => Some("Bypass"),
        _ => None,
    }
}

// ---- Ctrl-C ---------------------------------------------------------------------------------------------------------------

static SIGINTS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn on_sigint(_: libc::c_int) {
    SIGINTS.fetch_add(1, Ordering::SeqCst);
}

fn install_sigint() {
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
}

fn interrupts() -> usize {
    SIGINTS.load(Ordering::SeqCst)
}

// ---- keyboard -------------------------------------------------------------------------------------------------------------

enum Got {
    Line(String),
    Timeout,
    Eof,
}

/// Lines from stdin. The reading thread starts only when a line is first needed, so a one-shot question does not eat input.
struct Input {
    rx: Option<Receiver<String>>,
    can_start: bool,
    backlog: VecDeque<String>,
    eof: bool,
}

impl Input {
    fn stdin(can_start: bool) -> Input {
        Input { rx: None, can_start, backlog: VecDeque::new(), eof: false }
    }
    #[cfg(test)]
    fn from_channel(rx: Receiver<String>) -> Input {
        Input { rx: Some(rx), can_start: true, backlog: VecDeque::new(), eof: false }
    }
    fn start(&mut self) {
        if self.rx.is_none() && self.can_start {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                for l in std::io::stdin().lines() {
                    match l {
                        Ok(l) => {
                            if tx.send(l).is_err() {
                                return;
                            }
                        }
                        Err(_) => return,
                    }
                }
            });
            self.rx = Some(rx);
        }
    }
    fn available(&self) -> bool {
        self.can_start && !self.eof
    }
    fn recv(&mut self, wait: Duration) -> Got {
        if let Some(l) = self.backlog.pop_front() {
            return Got::Line(l);
        }
        let Some(rx) = &self.rx else { return Got::Eof };
        match rx.recv_timeout(wait) {
            Ok(l) => Got::Line(l),
            Err(RecvTimeoutError::Timeout) => Got::Timeout,
            Err(RecvTimeoutError::Disconnected) => {
                self.eof = true;
                Got::Eof
            }
        }
    }
    /// While an answer runs: look for `/stop`; other lines wait for the next prompt.
    fn poll_stop(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut stop = false;
        loop {
            match rx.try_recv() {
                Ok(l) if l.trim() == "/stop" => stop = true,
                Ok(l) => self.backlog.push_back(l),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.eof = true;
                    break;
                }
            }
        }
        stop
    }
}

// ---- one answer -----------------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Turn {
    Finished,
    Quit,
}

/// Runs one answer to its end: draws events, asks for approvals, stops on Ctrl-C (a second one gives up).
fn run_turn<O: Write, E: Write>(stream: &Stream, rend: &mut Renderer<O, E>, input: &mut Input, ints: &dyn Fn() -> usize) -> Turn {
    let base = ints();
    let mut stopping = false;
    let stop = |rend: &mut Renderer<O, E>, stopping: &mut bool| {
        if !*stopping {
            *stopping = true;
            if stream.reply(&stop_reply()).is_err() {
                rend.dim_err("(could not tell the service to stop)");
            } else {
                rend.dim_err("(stopping: Ctrl-C again to quit)");
            }
        }
    };
    loop {
        match ints().saturating_sub(base) {
            0 => {}
            1 => stop(rend, &mut stopping),
            _ => {
                let _ = stream.reply(&stop_reply());
                return Turn::Quit;
            }
        }
        if input.poll_stop() {
            stop(rend, &mut stopping);
        }
        match stream.rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(Event::Approval { id, tool, summary })) => {
                rend.event(&Event::Approval { id, tool, summary });
                let yes = ask_yes(rend, input, ints, base);
                let _ = stream.reply(&approve_reply(id, yes));
            }
            Ok(Ok(Event::Done)) => {
                rend.event(&Event::Done);
                return Turn::Finished;
            }
            Ok(Ok(e)) => rend.event(&e),
            Ok(Err(m)) => {
                rend.event(&Event::Error(m));
                return Turn::Finished;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                rend.event(&Event::Error("The connection closed before the answer was finished.".into()));
                return Turn::Finished;
            }
        }
    }
}

/// y/N on the terminal. No terminal to ask on, end of input or Ctrl-C all mean no.
fn ask_yes<O: Write, E: Write>(rend: &mut Renderer<O, E>, input: &mut Input, ints: &dyn Fn() -> usize, base: usize) -> bool {
    input.start();
    if !input.available() {
        rend.dim_err("(no terminal to ask on: denied. Run zo in a terminal to answer, or use --mode Bypass)");
        return false;
    }
    let _ = write!(rend.err, "Allow? [y/N] ");
    let _ = rend.err.flush();
    loop {
        if ints() > base {
            let _ = writeln!(rend.err);
            return false;
        }
        match input.recv(Duration::from_millis(100)) {
            Got::Line(l) => return is_yes(&l),
            Got::Eof => return false,
            Got::Timeout => {}
        }
    }
}

// ---- commands -------------------------------------------------------------------------------------------------------------

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn paint(on: bool, code: &str, s: &str) -> String {
    if on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// A text from the server's answer, safe to print (see `proto::clean`).
fn txt(v: &Value, k: &str) -> String {
    proto::clean(v.get(k).and_then(Value::as_str).unwrap_or(""))
}

fn list_chats(t: &Target) -> Result<(String, Vec<String>), String> {
    let r = t.op(&json!({"op": "chats"}))?;
    let items = arr(&r, "chats");
    if items.is_empty() {
        return Ok(("No chats yet.".into(), vec![]));
    }
    let now = now_ms();
    let mut out = String::new();
    let mut ids = vec![];
    for (n, c) in items.iter().enumerate() {
        let pin = if c["pinned"].as_bool() == Some(true) { "*" } else { " " };
        out += &format!("{:>3}{pin} {:<18} {:<12} {}\n", n + 1, txt(c, "id"), ago(now, c["updated"].as_u64().unwrap_or(0)), one_line(&txt(c, "title"), 60));
        ids.push(txt(c, "id").to_string());
    }
    Ok((out.trim_end().to_string(), ids))
}

fn show_chat(t: &Target, id: &str, color: bool, last: Option<usize>) -> Result<String, String> {
    let r = t.op(&json!({"op": "chat", "id": id}))?;
    let all = arr(&r, "entries");
    let skip = last.map(|n| all.len().saturating_sub(n)).unwrap_or(0);
    let mut out = format!("{}\n\n", paint(color, "1", &one_line(&txt(&r, "title"), 100)));
    for e in &all[skip..] {
        let text = txt(e, "text");
        out += &match txt(e, "kind").as_str() {
            "msg-user" => format!("{}\n\n", paint(color, "1", &format!("> {text}"))),
            "msg-ai" => format!("{text}\n\n"),
            "msg-error" => format!("{}\n", paint(color, "31", &format!("error: {text}"))),
            _ => format!("{}\n", paint(color, "2", &text)),
        };
    }
    Ok(out.trim_end().to_string())
}

fn list_memory(t: &Target) -> Result<String, String> {
    let r = t.op(&json!({"op": "memory"}))?;
    let items = arr(&r, "items");
    let mut out = String::new();
    if r["enabled"].as_bool() == Some(false) {
        out += "(memory is switched off)\n";
    }
    if items.is_empty() {
        return Ok(format!("{out}Nothing remembered yet."));
    }
    for i in items {
        out += &format!("{:<14}{} [{}] {}\n", txt(i, "id"), if i["pinned"].as_bool() == Some(true) { "*" } else { " " }, txt(i, "kind"), one_line(&txt(i, "text"), 200));
    }
    Ok(out.trim_end().to_string())
}

fn status(t: &Target) -> Result<String, String> {
    let r = t.op(&json!({"op": "status"}))?;
    let ready = r["ready"].as_bool() == Some(true);
    Ok(format!(
        "provider  {}\nmodel     {}\nmode      {}\nready     {}\nversion   {}",
        if ready { txt(&r, "provider") } else { "(none chosen)".to_string() },
        txt(&r, "model"),
        txt(&r, "mode"),
        if ready { "yes" } else { "no: choose a model with `zo use <provider> <model>`" },
        txt(&r, "version")
    ))
}

fn models(t: &Target, provider: Option<&str>) -> Result<String, String> {
    let p = t.op(&json!({"op": "providers"}))?;
    let mut out = String::from("Providers (* = current, key = has an API key or needs none):\n");
    for x in arr(&p, "providers") {
        out += &format!("  {} {:<14} {}{}\n", if txt(x, "id") == txt(&p, "current") { "*" } else { " " }, txt(x, "id"), txt(x, "name"), if x["has_key"].as_bool() == Some(true) { "" } else { "  (no key)" });
    }
    let mut req = json!({"op": "models"});
    if let Some(pr) = provider {
        req["provider"] = json!(pr);
    }
    let m = t.op(&req).map_err(|e| format!("{}\nModels of the provider could not be listed: {e}", out.trim_end()))?;
    out += &format!("\nModels of {} (* = current):\n", txt(&m, "provider"));
    for x in arr(&m, "models") {
        let name = proto::clean(&x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()));
        out += &format!("  {} {name}\n", if name == txt(&m, "current") { "*" } else { " " });
    }
    Ok(out.trim_end().to_string())
}

fn use_model(t: &Target, provider: Option<&str>, model: Option<&str>) -> Result<String, String> {
    let mut req = json!({"op": "select"});
    if let Some(p) = provider {
        req["provider"] = json!(p);
    }
    if let Some(m) = model {
        req["model"] = json!(m);
    }
    let r = t.op(&req)?;
    Ok(format!("Now using {} {}", txt(&r, "provider"), txt(&r, "model")))
}

fn mode(t: &Target, set: Option<&str>) -> Result<String, String> {
    let mut req = json!({"op": "mode"});
    if let Some(s) = set {
        req["set"] = json!(canonical_mode(s).ok_or("The mode must be Manual, Auto or Bypass.")?);
    }
    Ok(format!("Permission mode: {}", txt(&t.op(&req)?, "mode")))
}

/// Prints a command's text, or its error. Returns the exit code.
fn show(res: Result<String, String>) -> i32 {
    match res {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("zo: {e}");
            1
        }
    }
}

struct Ctx {
    target: Target,
    color_out: bool,
    color_err: bool,
    mode: Option<String>,
}

type Rend = Renderer<std::io::Stdout, std::io::Stderr>;

impl Ctx {
    fn renderer(&self) -> Rend {
        Renderer::new(std::io::stdout(), std::io::stderr(), self.color_err)
    }
}

/// One question. Returns the exit code.
fn ask(cx: &Ctx, text: &str, chat: Option<&str>, input: &mut Input) -> i32 {
    let mut rend = cx.renderer();
    let stream = match cx.target.send(&send_request(text, chat, cx.mode.as_deref())) {
        Ok(s) => s,
        Err(e) => {
            rend.error(&e);
            return 1;
        }
    };
    if run_turn(&stream, &mut rend, input, &interrupts) == Turn::Quit {
        return 130;
    }
    if std::io::stderr().is_terminal() {
        if let Some(c) = &rend.chat {
            rend.dim_err(&format!("(chat {c}: continue with `zo chat {c}`)"));
        }
    }
    if rend.failed { 1 } else { 0 }
}

const CHAT_HELP: &str = "/new            start a new chat\n/chats          list saved chats\n/open N         open number N of the last list (or a chat id)\n/model [name]   show the models, or choose one\n/mode [name]    show or set the permission mode (Manual, Auto, Bypass)\n/memory         list what ZO-AI remembers\n/stop           stop the running answer\n/quit           leave (also Ctrl-D, or Ctrl-C twice)";

fn chat_loop(cx: &Ctx, first: Option<String>, input: &mut Input) -> i32 {
    let mut chat = first;
    let mut listing: Vec<String> = vec![];
    let tty = std::io::stdin().is_terminal();
    let mut code = 0;
    if let Some(id) = &chat {
        match show_chat(&cx.target, id, cx.color_out, Some(6)) {
            Ok(s) => println!("{s}\n"),
            Err(e) => {
                eprintln!("zo: {e}");
                return 1;
            }
        }
    }
    input.start();
    let mut hinted = false;
    let base = interrupts();
    let mut seen = base;
    loop {
        if tty {
            print!("{}", paint(cx.color_out, "1;36", "› "));
            let _ = std::io::stdout().flush();
        }
        // wait for a line; Ctrl-C at the prompt asks to press it again
        let line = loop {
            if interrupts() > seen {
                if hinted {
                    println!();
                    return code;
                }
                hinted = true;
                seen = interrupts();
                eprintln!("\n(Ctrl-C again, Ctrl-D or /quit to leave)");
                if tty {
                    print!("{}", paint(cx.color_out, "1;36", "› "));
                    let _ = std::io::stdout().flush();
                }
            }
            match input.recv(Duration::from_millis(100)) {
                Got::Line(l) => break l,
                Got::Eof => {
                    if tty {
                        println!();
                    }
                    return code;
                }
                Got::Timeout => {}
            }
        };
        hinted = false;
        seen = interrupts();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(cmd) = line.strip_prefix('/') {
            let (name, arg) = cmd.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((cmd, ""));
            let arg_opt = Some(arg).filter(|a| !a.is_empty());
            match name {
                "quit" | "exit" | "q" => return code,
                "help" | "?" => println!("{CHAT_HELP}"),
                "new" => {
                    chat = None;
                    eprintln!("(new chat)");
                }
                "chats" => match list_chats(&cx.target) {
                    Ok((s, ids)) => {
                        println!("{s}");
                        listing = ids;
                    }
                    Err(e) => eprintln!("zo: {e}"),
                },
                "open" => {
                    if listing.is_empty() {
                        listing = list_chats(&cx.target).map(|x| x.1).unwrap_or_default();
                    }
                    let id = match arg.parse::<usize>() {
                        Ok(n) if n >= 1 && n <= listing.len() => listing[n - 1].clone(),
                        _ if !arg.is_empty() && arg.parse::<usize>().is_err() => arg.to_string(),
                        _ => {
                            eprintln!("zo: no chat number {arg}: use /chats to see the list");
                            continue;
                        }
                    };
                    match show_chat(&cx.target, &id, cx.color_out, Some(6)) {
                        Ok(s) => {
                            println!("{s}\n");
                            chat = Some(id);
                        }
                        Err(e) => eprintln!("zo: {e}"),
                    }
                }
                "model" => match arg_opt {
                    None => match models(&cx.target, None) {
                        Ok(s) => println!("{s}"),
                        Err(e) => eprintln!("zo: {e}"),
                    },
                    Some(m) => match use_model(&cx.target, None, Some(m)) {
                        Ok(s) => println!("{s}"),
                        Err(e) => eprintln!("zo: {e}"),
                    },
                },
                "mode" => match mode(&cx.target, arg_opt) {
                    Ok(s) => println!("{s}"),
                    Err(e) => eprintln!("zo: {e}"),
                },
                "memory" => match list_memory(&cx.target) {
                    Ok(s) => println!("{s}"),
                    Err(e) => eprintln!("zo: {e}"),
                },
                "stop" => eprintln!("(nothing is running)"),
                _ => eprintln!("zo: unknown command /{name} (try /help)"),
            }
            continue;
        }
        let mut rend = cx.renderer();
        let stream = match cx.target.send(&send_request(line, chat.as_deref(), cx.mode.as_deref())) {
            Ok(s) => s,
            Err(e) => {
                rend.error(&e);
                code = 1;
                continue;
            }
        };
        if run_turn(&stream, &mut rend, input, &interrupts) == Turn::Quit {
            return 130;
        }
        if let Some(c) = rend.chat.take() {
            chat = Some(c);
        }
        if rend.failed {
            code = 1;
        }
        if rend.tokens != (0, 0) {
            rend.dim_err(&format!("({} tokens in, {} out)", rend.tokens.0, rend.tokens.1));
        }
        println!();
        seen = interrupts();
    }
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let o = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("zo: {e}");
            return 2;
        }
    };
    if o.help {
        println!("{USAGE}");
        return 0;
    }
    if o.version {
        println!("zo {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    install_sigint();
    let no_color = o.no_color || std::env::var_os("NO_COLOR").map(|v| !v.is_empty()).unwrap_or(false);
    let color_out = !no_color && std::io::stdout().is_terminal();
    let color_err = !no_color && std::io::stderr().is_terminal();
    if let Some(m) = &o.mode {
        if canonical_mode(m).is_none() {
            eprintln!("zo: the mode must be Manual, Auto or Bypass");
            return 2;
        }
    }
    let remote = if o.local { None } else { o.remote.clone().or_else(|| std::env::var("ZO_REMOTE_URL").ok()).map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) };
    let target = match remote {
        Some(url) => {
            // the same rule as the desktop sync: https, or http only for this computer itself (the token goes with every call)
            let url = match zo_core::sync::clean_url(&url) {
                Ok(u) => u,
                Err(_) => {
                    eprintln!("zo: the server address must start with https:// (http only for localhost); got {}", proto::one_line(&url, 80));
                    return 2;
                }
            };
            let token = match read_token(o.token_file.as_deref(), o.token_env.as_deref().unwrap_or("ZO_ACCESS_TOKEN"), &|n| std::env::var(n).ok()) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("zo: {e}");
                    return 1;
                }
            };
            Target::Remote { url, token }
        }
        None => Target::Local(zo_core::service::socket_path()),
    };
    let cx = Ctx { target, color_out, color_err, mode: o.mode.clone().and_then(|m| canonical_mode(&m).map(str::to_string)) };
    let stdin_tty = std::io::stdin().is_terminal();
    let rest = &o.rest;
    let cmd = rest.first().map(String::as_str).unwrap_or("");
    let arg1 = rest.get(1).map(String::as_str);
    let usage = |m: &str| {
        eprintln!("zo: {m} (try --help)");
        2
    };
    match cmd {
        "" => {
            if stdin_tty {
                eprintln!("{USAGE}");
                return 2;
            }
            ask_from_stdin(&cx, None, &o)
        }
        "chat" => chat_loop(&cx, arg1.map(str::to_string).or(o.chat.clone()), &mut Input::stdin(true)),
        "chats" => show(list_chats(&cx.target).map(|x| x.0)),
        "show" => match arg1 {
            Some(id) => show(show_chat(&cx.target, id, color_out, None)),
            None => usage("show needs a chat id"),
        },
        "memory" => show(list_memory(&cx.target)),
        "remember" => {
            if rest.len() < 2 {
                return usage("remember needs the text to remember");
            }
            show(cx.target.op(&json!({"op": "memory_add", "text": rest[1..].join(" ")})).map(|r| if r["new"].as_bool() == Some(false) { "Already remembered.".to_string() } else { "Remembered.".to_string() }))
        }
        "forget" => match arg1 {
            Some(id) => show(cx.target.op(&json!({"op": "memory_forget", "id": id})).map(|_| format!("Forgot {id} (if it existed)."))),
            None => usage("forget needs a note id (see `zo memory`)"),
        },
        "status" => show(status(&cx.target)),
        "models" => show(models(&cx.target, arg1)),
        "use" => match arg1 {
            Some(p) => show(use_model(&cx.target, Some(p), rest.get(2).map(String::as_str))),
            None => usage("use needs a provider (see `zo models`) and optionally a model"),
        },
        "mode" => match arg1 {
            Some(m) if canonical_mode(m).is_none() => usage("the mode must be Manual, Auto or Bypass"),
            _ => show(mode(&cx.target, arg1)),
        },
        "ssh" => {
            eprintln!("zo: machines over SSH are not available in this client yet");
            1
        }
        "ask" => {
            if rest.len() < 2 && stdin_tty {
                return usage("ask needs the question");
            }
            ask_text_or_stdin(&cx, &rest[1..], &o)
        }
        _ => ask_text_or_stdin(&cx, rest, &o),
    }
}

fn ask_text_or_stdin(cx: &Ctx, words: &[String], o: &Opts) -> i32 {
    let q = if words.is_empty() { None } else { Some(words.join(" ")) };
    if std::io::stdin().is_terminal() {
        return ask(cx, q.as_deref().unwrap_or(""), o.chat.as_deref(), &mut Input::stdin(true));
    }
    // a question on the command line is the whole question, unless --stdin asks to add the piped input (reading it would
    // hang a script that merely inherited an open, empty stdin)
    match q {
        Some(q) if !o.stdin => ask(cx, &q, o.chat.as_deref(), &mut Input::stdin(false)),
        q => ask_from_stdin(cx, q, o),
    }
}

/// Stdin is piped: it is the question, or is added to the one on the command line.
fn ask_from_stdin(cx: &Ctx, question: Option<String>, o: &Opts) -> i32 {
    let mut piped = String::new();
    if std::io::stdin().read_to_string(&mut piped).is_err() {
        eprintln!("zo: cannot read the input");
        return 1;
    }
    let text = match (question, piped.trim()) {
        (Some(q), "") => q,
        (Some(q), p) => format!("{q}\n\n{p}"),
        (None, p) => p.to_string(),
    };
    if text.trim().is_empty() {
        eprintln!("zo: no question given (try --help)");
        return 2;
    }
    // stdin is used up: an approval cannot be asked, so it is denied
    ask(cx, &text, o.chat.as_deref(), &mut Input::stdin(false))
}

fn main() {
    std::process::exit(run());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_words() {
        let o = parse_args(&a(&["--remote", "https://x.test", "--token-file=/t", "-c", "c1", "what", "is", "rust"])).unwrap();
        assert_eq!(o.remote.as_deref(), Some("https://x.test"));
        assert_eq!(o.token_file.as_deref(), Some("/t"));
        assert_eq!(o.chat.as_deref(), Some("c1"));
        assert_eq!(o.rest, a(&["what", "is", "rust"]));
        // after -- everything is text
        let o = parse_args(&a(&["--", "-h", "me"])).unwrap();
        assert!(!o.help);
        assert_eq!(o.rest, a(&["-h", "me"]));
        assert!(parse_args(&a(&["--remote"])).is_err());
        assert!(parse_args(&a(&["--bogus"])).is_err());
    }

    #[test]
    fn a_token_as_an_argument_is_refused() {
        for bad in [&["--token", "abc"][..], &["--token=abc"][..], &["x", "--access-token", "abc"][..]] {
            let e = parse_args(&a(bad)).unwrap_err();
            assert!(e.contains("Refusing a device token"), "{e}");
            assert!(!e.contains("abc"));
        }
    }

    #[test]
    fn the_token_comes_from_a_file_or_the_environment() {
        let dir = std::env::temp_dir().join(format!("zo-cli-token-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("t");
        std::fs::write(&f, "  zd_from_file_0123456789abc\n").unwrap();
        let env = |n: &str| if n == "ZO_ACCESS_TOKEN" { Some("zd_from_env_0123456789abcd".to_string()) } else { None };
        assert_eq!(read_token(f.to_str(), "ZO_ACCESS_TOKEN", &env).unwrap().as_deref(), Some("zd_from_file_0123456789abc"));
        assert_eq!(read_token(None, "ZO_ACCESS_TOKEN", &env).unwrap().as_deref(), Some("zd_from_env_0123456789abcd"));
        assert_eq!(read_token(None, "OTHER", &env).unwrap(), None);
        std::fs::write(&f, "\n").unwrap();
        assert!(read_token(f.to_str(), "X", &env).is_err());
        assert!(read_token(Some("/no/such/file"), "X", &env).is_err());
        // a token that is not a device token is refused with a clear message
        let old = |_: &str| Some("old-global-token-value-123456".to_string());
        assert!(read_token(None, "ZO_ACCESS_TOKEN", &old).unwrap_err().contains("zd_"));
        std::fs::write(&f, "nope\n").unwrap();
        assert!(read_token(f.to_str(), "X", &env).unwrap_err().contains("zd_"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn modes_are_normalised() {
        assert_eq!(canonical_mode("auto"), Some("Auto"));
        assert_eq!(canonical_mode(" BYPASS "), Some("Bypass"));
        assert_eq!(canonical_mode("yolo"), None);
    }

    /// A stream fed by the test, which records what the client answers.
    fn fake(events: Vec<Event>) -> (Stream, Arc<Mutex<Vec<Value>>>) {
        let (tx, rx) = mpsc::channel();
        for e in events {
            tx.send(Ok(e)).unwrap();
        }
        let sent = Arc::new(Mutex::new(vec![]));
        let s2 = sent.clone();
        // keep the sender alive inside the reply closure, so the channel is not seen as closed
        let reply: Box<dyn Fn(&Value) -> Result<(), String> + Send> = Box::new(move |v| {
            let _keep = &tx;
            s2.lock().unwrap().push(v.clone());
            Ok(())
        });
        (Stream::new(rx, reply), sent)
    }

    fn rend() -> Renderer<Vec<u8>, Vec<u8>> {
        Renderer::new(Vec::new(), Vec::new(), false)
    }

    #[test]
    fn an_approval_is_asked_and_answered() {
        for (typed, want) in [("y", true), ("n", false), ("", false)] {
            let (s, sent) = fake(vec![Event::Approval { id: 9, tool: "shell".into(), summary: "rm x".into() }, Event::Delta("ok".into()), Event::Done]);
            let (tx, rx) = mpsc::channel();
            tx.send(typed.to_string()).unwrap();
            let mut input = Input::from_channel(rx);
            let mut r = rend();
            assert_eq!(run_turn(&s, &mut r, &mut input, &|| 0), Turn::Finished);
            assert_eq!(sent.lock().unwrap()[0], json!({"op": "approve", "id": 9, "yes": want}));
            assert!(String::from_utf8_lossy(&r.err).contains("Allow shell?"));
            assert_eq!(String::from_utf8_lossy(&r.out), "ok\n");
        }
    }

    #[test]
    fn without_a_terminal_an_approval_is_denied() {
        let (s, sent) = fake(vec![Event::Approval { id: 1, tool: "shell".into(), summary: "x".into() }, Event::Done]);
        let mut input = Input::stdin(false);
        let mut r = rend();
        run_turn(&s, &mut r, &mut input, &|| 0);
        assert_eq!(sent.lock().unwrap()[0], json!({"op": "approve", "id": 1, "yes": false}));
        assert!(String::from_utf8_lossy(&r.err).contains("denied"));
    }

    /// A Ctrl-C counter that reads 0 at the start of the turn and `later` afterwards.
    fn counter(later: usize) -> impl Fn() -> usize {
        let calls = AtomicUsize::new(0);
        move || if calls.fetch_add(1, Ordering::SeqCst) == 0 { 0 } else { later }
    }

    #[test]
    fn ctrl_c_stops_and_a_second_one_quits() {
        // first: stop is sent, the answer then ends on its own
        let (s, sent) = fake(vec![Event::Stopped, Event::Done]);
        let mut r = rend();
        assert_eq!(run_turn(&s, &mut r, &mut Input::stdin(false), &counter(1)), Turn::Finished);
        assert_eq!(sent.lock().unwrap()[0], json!({"op": "stop"}));
        assert!(String::from_utf8_lossy(&r.err).contains("(stopped)"));
        // second: the turn gives up although the answer never ends
        let (s, sent) = fake(vec![Event::Delta("a".into())]);
        let mut r = rend();
        assert_eq!(run_turn(&s, &mut r, &mut Input::stdin(false), &counter(2)), Turn::Quit);
        assert!(sent.lock().unwrap().iter().any(|v| v["op"] == "stop"));
    }

    #[test]
    fn a_broken_connection_is_an_error() {
        let (tx, rx) = mpsc::channel();
        tx.send(Err("gone".to_string())).unwrap();
        let s = Stream::new(rx, Box::new(|_| Ok(())));
        let mut r = rend();
        run_turn(&s, &mut r, &mut Input::stdin(false), &|| 0);
        assert!(r.failed);
    }
}
