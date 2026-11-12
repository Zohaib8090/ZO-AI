//! The loop that makes a model useful: ask, run the tools it calls (through the same permission policy and
//! approval as the MCP server), give it the results, and repeat until it answers in words.

use super::providers::{self, Protocol, Provider};
use std::io::BufRead;
use super::wire::{self, Reply, ToolResult, Turn, Usage};
use super::Selection;
use crate::approval::Approver;
use crate::policy::Mode;
use crate::{mcp, mcp_client};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAX_STEPS: usize = 15;

thread_local! {
    /// When the answer that is being made on this thread must be finished (restricted tenants only).
    static DEADLINE: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// What ZO-AI is told about itself: the full text, or for a restricted tenant the short one for the web.
pub fn system_prompt() -> &'static str {
    if crate::tenant::is_restricted() {
        RESTRICTED_PROMPT
    } else {
        SYSTEM_PROMPT
    }
}

pub const RESTRICTED_PROMPT: &str = "You are ZO-AI: the AI assistant of Zohara OS, here running on the web. \
You are an AI model; whatever model is behind you (the person brings their own API key), here you are ZO-AI.\n\
\n\
WHAT YOU CAN DO\n\
You run on a server and have no access to the person's computer: you cannot read or change their files, run commands, take screenshots or control their system. If they ask for that, say that the web version cannot do it and that the ZO-AI desktop app on their own computer can (it keeps their chats in sync). \
You can search the web with zohara_web_search, read a public web page with zohara_web_fetch, and keep a long-term memory of the person: the notes you are given at the start are from earlier chats. \
When they ask you to remember or forget something, use zohara_memory_save and zohara_memory_forget; to look for something from before, use zohara_memory_search.\n\
\n\
THE WEB\n\
For anything that may have changed or that you are not sure of (news, versions, prices, documentation, \"what is ...\"), search first, then read the best result, and say where the answer came from.\n\
\n\
HOW TO WORK\n\
Report what really happened: if a tool failed, say so and say why. Do not invent results or tool names, and do not claim to have done something you have no tool for. \
Never save passwords, keys or other secrets to memory, and never print secrets you come across. \
If a request is unclear, ask one short question instead of guessing. \
Answer in the language the person writes in, in plain, short sentences.";

pub const SYSTEM_PROMPT: &str = "You are ZO-AI: the AI assistant and agent built into Zohara OS (Arch Linux with KDE Plasma, on Wayland). \
You are an AI model working as an agent on this computer, for the person who is signed in. You are not a person, and you are not a website: you live on this machine and can act on it. \
Whatever model is behind you (the person chooses it), here you are ZO-AI.\n\
\n\
WHAT YOU CAN DO\n\
Your tools let you read and change files, run shell commands in a terminal, take screenshots, list and stop processes, work with system snapshots, and control Android (adb) and Waydroid. \
You can also manage ZO-AI itself: projects and their memory, scheduled tasks, saved chats, and the MCP servers that are added to you. \
This system uses pacman (and the AUR), Flatpak, and Zohara Settings and Zohara Store for updates and apps.\n\
\n\
OTHER COMPUTERS (SSH)\n\
The person can add machines (a cloud VM, a server) in Settings > Machines. See them with zohara_ssh_machines and work on them with zohara_ssh_run, zohara_ssh_read_file and zohara_ssh_write_file; each command asks the person first when it must. \
When they ask you to do a coding job on a machine, you can start Claude Code there with zohara_ssh_run and a command like claude -p \"<the task>\" (give it a long enough timeout), if it is installed on that machine. Never copy secrets between machines.\n\
\n\
THE WEB AND YOUR MEMORY\n\
For anything that may have changed or that you are not sure of (news, versions, prices, documentation, \"what is ...\"), search first with zohara_web_search, then read the best result with zohara_web_fetch, and say where the answer came from. Use the built-in browser only for pages that need a sign-in or clicks. \
You have a long-term memory of the person that is shared with their phone: the notes you are given at the start are from earlier chats. When they ask you to remember or forget something, use zohara_memory_save and zohara_memory_forget; to look for something from an earlier chat, use zohara_memory_search and zohara_ai_search_chats.\n\
\n\
MCP SERVERS ARE FOR YOU\n\
An MCP server gives you more tools. When the person gives you an MCP server (an https address, or a repository that runs one) and asks you to install, add or connect it, it is for YOU, and you add it to yourself with zohara_ai_add_mcp_server (or zohara_ai_add_mcp_config when you have a JSON configuration from a README). \
Do not tell them to add it to Claude, ChatGPT, Grok or some other assistant, unless they ask for that. You are an AI model too; it is your own tool. \
A remote server: pass its https address (a bare site address is fine, the /mcp part is found for you). If it asks for a sign-in, the person's browser opens and they sign in; say that this will happen. \
A repository to run locally: clone it, read its README, install what it needs, then add the command that starts it over standard input and output. \
Never say a server is \"already running, nothing to install\" and stop there: if it is not yet in your tools, add it. After adding, its tools are yours from the next message; tell the person that.\n\
\n\
TERMINALS, BROWSER AND SUB-AGENTS (when you have these tools)\n\
The ZO-AI window has a side panel with terminals and a full browser that the person can see and use too. \
Terminals (zohara_term_open, zohara_term_run, zohara_term_send, zohara_term_read, zohara_term_list, zohara_term_close): they are real shells. \
Use them when the person asks you to open or use a terminal, and for things that keep running (a dev server, a watcher, an installer that asks questions): open one terminal per job, give it a name, and read it again later with zohara_term_read. \
For a quick one-off command the shell tool is enough. Use zohara_term_send to answer a prompt or press a key (Ctrl+C stops a running program). \
Browser (zohara_browser_open, _read, _click, _type, _scroll, _back, _screenshot, _js): use it to look things up on the web and to use websites for the person. \
Open a page, read it (the result numbers the links and buttons), then click or type by number, and read again. Do not guess addresses you are not sure of: search instead. \
Never type passwords, card numbers or other secrets into a page for the person, and do not submit orders, payments, posts or messages without asking first. \
Sub-agents (zohara_ai_subagent): for a self-contained side job or a long chore, hand it to a sub-agent with a complete description (it does not see this conversation) and use its report. \
Several separate jobs are several calls. A sub-agent cannot start sub-agents. Do small things yourself.\n\
\n\
HOW TO WORK\n\
Look before you change. If a job needs something that none of your specific tools does, do it with the shell tool (it asks the person first when it must) rather than telling the person to run commands themselves; only hand a step to the person when it truly needs them (a password, a click in another program). Say briefly what you are about to do, then do it. When you run commands, the app shows them in a small collapsed box, so do not paste raw terminal output into your answer; tell the person what you found in a few plain words, and quote only the lines that matter. \
Risky tools ask the person first; if they refuse, stop and say so. Never try to get around a refusal or a permission. \
Report what really happened: if a command failed, say so and say why. Do not claim something worked unless a tool result shows it. Do not invent file contents, results or tool names. \
Do not delete or overwrite things the person did not ask you to; prefer the reversible way (snapshots exist). \
Keep the person's secrets out of your answers: never print passwords, tokens or keys you come across. \
If a request is unclear, ask one short question instead of guessing. \
Answer in the language the person writes in, in plain, short sentences.";

pub const VISION_YES: &str = "SEEING PICTURES\nYou can look at pictures. A screenshot you take with zohara_ss is shown to you with the result, and any picture file (png, jpg, webp, gif) \
can be shown to you with zohara_view_image. Pictures that the person puts in a message are shown to you too. Describe what you actually see, and say so when something in a picture is unclear. \
Never tell the person that you cannot see images or screenshots.";
pub const VISION_NO: &str = "SEEING PICTURES\nThe model you run on cannot look at pictures, so a screenshot is only saved as a file and you cannot see what is in it. Say so plainly if asked to look at one, \
and suggest that the person picks a model that can see images (for example Claude, Gemini or GPT-4o) in the model settings. Do not pretend to have seen a picture.";

pub struct Config<'a> {
    pub provider: &'a Provider,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
    pub system: String,
    /// Can the model look at pictures? Then screenshots and pictures in a message are sent to it.
    pub vision: bool,
}

impl Config<'static> {
    /// The saved choice, with its key from the keyring.
    pub fn from_selection(sel: &Selection, system: &str) -> Result<Config<'static>, String> {
        let provider = providers::find(&sel.provider).ok_or("No provider is chosen yet.")?;
        if sel.model.is_empty() {
            return Err("No model is chosen yet.".into());
        }
        let restricted = crate::tenant::is_restricted();
        if restricted {
            check_restricted_provider(provider, &sel.base_url_for(provider))?;
        }
        let key = super::profiles::get_key(sel, provider.id);
        if key.is_none() && !provider.local {
            return Err(format!("There is no API key for {} yet. Add one in the model settings.", provider.name));
        }
        let vision = sel.vision.unwrap_or_else(|| providers::can_see(provider, &sel.model));
        if restricted {
            // no pictures from files on a server, and nothing about screenshots
            return Ok(Config { provider, base_url: sel.base_url_for(provider), model: sel.model.clone(), key, system: system.to_string(), vision: false });
        }
        let note = if vision {
            VISION_YES
        } else {
            VISION_NO
        };
        Ok(Config { provider, base_url: sel.base_url_for(provider), model: sel.model.clone(), key, system: format!("{system}\n\n{note}"), vision })
    }
}

pub enum Event {
    /// A new piece of the answer, as it is being written.
    TextDelta(String),
    /// The whole answer of one step, when it is complete.
    Text(String),
    ToolStart { name: String, summary: String },
    ToolDone { name: String, ok: bool, output: String },
    /// What the last request used (tokens in, out, from the provider's cache).
    Usage(Usage),
}

/// What a restricted tenant may reach: no Claude Code, and only a model server with a public https address.
fn check_restricted_provider(p: &Provider, base_url: &str) -> Result<(), String> {
    if p.protocol == Protocol::ClaudeCode {
        return Err("Claude Code is not available to this account.".into());
    }
    #[cfg(test)]
    if local_allowed() {
        return Ok(());
    }
    crate::tools::web::public_https_url(base_url)
}

/// Tests only: lets some restricted tenants (by id) talk to a fake model server on this computer.
#[cfg(test)]
pub(crate) static ALLOW_LOCAL_TENANTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) fn test_allow_local(id: &str) {
    ALLOW_LOCAL_TENANTS.lock().unwrap_or_else(|e| e.into_inner()).push(id.to_string());
}

#[cfg(test)]
fn local_allowed() -> bool {
    crate::tenant::current().is_some_and(|t| ALLOW_LOCAL_TENANTS.lock().unwrap_or_else(|e| e.into_inner()).contains(&t.id))
}

fn agent() -> ureq::Agent {
    if crate::tenant::is_restricted() {
        #[cfg(test)]
        if local_allowed() {
            return ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(crate::tenant::max_answer()).redirects(0).build();
        }
        return crate::tools::web::public_agent(Duration::from_secs(10), crate::tenant::max_answer());
    }
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(180)).build()
}

fn http_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body).ok().and_then(|v| v.get("error").map(wire::error_text)).unwrap_or_else(|| body.chars().take(300).collect());
            format!("The provider answered {code}: {msg}")
        }
        other => format!("Could not reach the provider: {other}"),
    }
}

enum Chunk {
    Delta(String),
    Done(Result<Reply, String>),
}

/// Sends the request and reads the answer as it is written, handing each new piece of text on.
fn stream_request(req: wire::Request, protocol: Protocol, tx: &std::sync::mpsc::Sender<Chunk>) -> Result<Reply, String> {
    let mut r = agent().post(&req.url);
    for (k, v) in &req.headers {
        r = r.set(k, v);
    }
    let resp = r.send_json(req.body).map_err(http_error)?;
    let streaming = resp.header("content-type").is_some_and(|c| c.contains("text/event-stream"));
    if !streaming {
        // a server that answers all at once
        let v: Value = resp.into_json().map_err(|e| format!("The answer was not valid JSON: {e}"))?;
        return wire::parse(protocol, &v);
    }
    let mut acc = wire::StreamAcc::default();
    for line in std::io::BufReader::new(resp.into_reader()).lines() {
        let line = line.map_err(|e| format!("The connection broke while the answer was coming: {e}"))?;
        if let Some(d) = acc.feed(protocol, &line)? {
            let _ = tx.send(Chunk::Delta(d));
        }
    }
    Ok(acc.finish())
}

fn ask(cfg: &Config, turns: &[Turn], specs: &[wire::ToolSpec], cancel: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Result<Reply, String> {
    if crate::tenant::is_restricted() {
        check_restricted_provider(cfg.provider, &cfg.base_url)?;
    }
    let mut req = wire::build(cfg.provider, &cfg.base_url, cfg.key.as_deref(), &cfg.model, &cfg.system, turns, specs);
    let protocol = cfg.provider.protocol;
    wire::make_streaming(&mut req, protocol);
    // the request runs on its own thread so Stop works while the model is still thinking
    let (tx, rx) = std::sync::mpsc::channel();
    crate::tenant::spawn(move || {
        let out = stream_request(req, protocol, &tx);
        let _ = tx.send(Chunk::Done(out));
    });
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Chunk::Delta(t)) => on_event(Event::TextDelta(t)),
            Ok(Chunk::Done(r)) => return r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(STOPPED.into());
                }
                if DEADLINE.with(|d| d.get()).is_some_and(|t| std::time::Instant::now() > t) {
                    return Err(too_slow());
                }
            }
            Err(_) => return Err("The request ended without an answer.".into()),
        }
    }
}

fn too_slow() -> String {
    format!("The answer took longer than {} seconds, so it was stopped.", crate::tenant::max_answer().as_secs())
}

/// Run one user message to the end. `history` gets every step appended, so it can be saved and continued.
pub const STOPPED: &str = "Stopped.";

/// One plain question to the model, no tools: the answer as text. Used for small jobs such as the memory agent.
pub fn complete(cfg: &Config, system: &str, prompt: &str, cancel: &AtomicBool) -> Result<String, String> {
    let c = Config { provider: cfg.provider, base_url: cfg.base_url.clone(), model: cfg.model.clone(), key: cfg.key.clone(), system: system.to_string(), vision: false };
    Ok(ask(&c, &[Turn::User(prompt.to_string())], &[], cancel, &mut |_| {})?.text)
}

pub fn run(cfg: &Config, history: &mut Vec<Turn>, user: &str, mode: Mode, approver: &dyn Approver, cancel: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Result<(), String> {
    let restricted = crate::tenant::is_restricted();
    crate::tools::begin_answer();
    if restricted {
        if user.chars().count() > crate::tenant::MAX_MESSAGE_CHARS {
            return Err(format!("That message is too long (at most {} characters).", crate::tenant::MAX_MESSAGE_CHARS));
        }
        check_restricted_provider(cfg.provider, &cfg.base_url)?;
        crate::tenant::check_room()?;
    }
    let max_steps = if restricted { crate::tenant::MAX_STEPS } else { MAX_STEPS };
    let _deadline = restricted.then(|| DeadlineGuard::set(std::time::Instant::now() + crate::tenant::max_answer()));
    let restricted_cfg;
    let cfg = if restricted && cfg.system.contains(SYSTEM_PROMPT) {
        // whoever built the settings, a restricted account is never told it has a computer to control
        restricted_cfg = Config { provider: cfg.provider, base_url: cfg.base_url.clone(), model: cfg.model.clone(), key: cfg.key.clone(), system: cfg.system.replacen(SYSTEM_PROMPT, RESTRICTED_PROMPT, 1), vision: false };
        &restricted_cfg
    } else {
        cfg
    };
    // pictures named in the message (a path the person added with "Add a file", say) go to a model that can see
    let pics: Vec<wire::ImageRef> = if cfg.vision && !restricted { user.split_whitespace().map(|w| w.trim_matches(|c| "\"'`()[]{}<>,;:!?".contains(c))).filter_map(wire::ImageRef::from_path).collect() } else { Vec::new() };
    history.push(if pics.is_empty() { Turn::User(user.to_string()) } else { Turn::UserImages(user.to_string(), pics) });
    // ZO-AI's tools, and those of the MCP servers added in Settings (started for this turn only)
    // a restricted account has no MCP servers: they are programs on the server
    let mut external = if restricted { mcp_client::External::start_with(&[]) } else { mcp_client::External::start() };
    let mut specs = wire::builtin_specs();
    // a sub-agent cannot start sub-agents
    if DEPTH.with(|d| d.get()) > 0 {
        specs.retain(|t| t.name != "zohara_ai_subagent");
    }
    specs.extend(external.tools().iter().map(|t| wire::ToolSpec { name: t.public_name(), description: t.description.clone(), schema: t.schema.clone() }));
    // what the connected servers say the model should know about them
    let notes = external.instructions();
    let problem_note = external.problems_note();
    let cfg_with_notes;
    let cfg = if notes.is_empty() && problem_note.is_none() {
        cfg
    } else {
        let mut extra = String::new();
        if !notes.is_empty() {
            let list = notes.iter().map(|(server, text)| format!("[{server}] {text}")).collect::<Vec<_>>().join("\n");
            extra.push_str(&format!("\n\nNOTES FROM THE MCP SERVERS YOU HAVE (they describe themselves):\n{list}"));
        }
        if let Some(p) = &problem_note {
            extra.push_str(&format!("\n\n{p}"));
        }
        cfg_with_notes = Config { provider: cfg.provider, base_url: cfg.base_url.clone(), model: cfg.model.clone(), key: cfg.key.clone(), system: format!("{}{extra}", cfg.system), vision: cfg.vision };
        &cfg_with_notes
    };
    for p in &external.problems {
        on_event(Event::ToolDone { name: "mcp".into(), ok: false, output: format!("An MCP server did not start: {p}") });
    }
    // a conversation that has grown close to the end of the window is summarized first, so there is room to go on
    let window = super::context::window_for(cfg.provider, &cfg.model);
    let used = super::context::tokens(&cfg.system) + super::context::specs_tokens(&specs) + super::context::history_tokens(history);
    if used as f64 > window as f64 * super::context::COMPACT_AT {
        match compact(cfg, history, cancel, &mut |_| {}) {
            Ok(()) => on_event(Event::ToolDone { name: "compaction".into(), ok: true, output: "The earlier part of this conversation was summarized, so there is room to go on.".into() }),
            Err(e) if e == STOPPED => return Err(e),
            Err(_) => {} // too short to summarize, or the summary could not be made: carry on and let the provider decide
        }
    }
    for _ in 0..max_steps {
        if cancel.load(Ordering::Relaxed) {
            return Err(STOPPED.into());
        }
        if DEADLINE.with(|d| d.get()).is_some_and(|t| std::time::Instant::now() > t) {
            return Err(too_slow());
        }
        let reply = match ask(cfg, history, &specs, cancel, &mut *on_event) {
            // a model that takes no pictures answers with an error: drop the pictures from the conversation and ask again once
            Err(e) if has_images(history) && looks_like_picture_error(&e) => {
                strip_images(history);
                on_event(Event::ToolDone { name: "vision".into(), ok: false, output: "This model does not accept pictures, so they were left out.".into() });
                ask(cfg, history, &specs, cancel, &mut *on_event)?
            }
            other => other?,
        };
        if let Some(u) = reply.usage {
            on_event(Event::Usage(u));
        }
        if !reply.text.is_empty() {
            on_event(Event::Text(reply.text.clone()));
        }
        history.push(Turn::Assistant { text: reply.text, calls: reply.calls.clone() });
        if reply.calls.is_empty() {
            return Ok(());
        }
        let mut results = Vec::new();
        for call in reply.calls {
            if cancel.load(Ordering::Relaxed) {
                return Err(STOPPED.into());
            }
            on_event(Event::ToolStart { name: call.name.clone(), summary: mcp::summarize(&call.name, &call.args) });
            let outcome = if restricted && !crate::tools::allowed_for_restricted(&call.name) {
                Err(crate::tools::refusal(&call.name))
            } else if call.name == "zohara_ai_subagent" && crate::term::enabled() {
                subagent(cfg, &call.args, mode, approver, cancel)
            } else if external.owns(&call.name) { external.execute(&call.name, &call.args, mode, approver) } else { mcp::execute(&call.name, &call.args, mode, approver) };
            let (content, is_error) = match outcome {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            // a screenshot, or a picture the model asked to see: sent along when the model can look at it
            let mut images = Vec::new();
            let mut content = content;
            if !is_error && (call.name == "zohara_ss" || call.name == "zohara_view_image" || call.name == "zohara_browser_screenshot") {
                match (cfg.vision, wire::ImageRef::from_path(content.trim())) {
                    (true, Some(img)) => {
                        content = format!("{} (the picture is attached)", content.trim());
                        images.push(img);
                    }
                    (true, None) => content = format!("{} (this file is too large, or not a picture the model can take, so it could not be shown)", content.trim()),
                    (false, _) => content = format!("{} (saved; this model cannot look at pictures)", content.trim()),
                }
            }
            on_event(Event::ToolDone { name: call.name.clone(), ok: !is_error, output: content.clone() });
            results.push(ToolResult { id: call.id, name: call.name, content, is_error, images });
        }
        history.push(Turn::Results(results));
    }
    Err(format!("Stopped after {max_steps} steps without a final answer."))
}

struct DeadlineGuard(Option<std::time::Instant>);

impl DeadlineGuard {
    fn set(t: std::time::Instant) -> DeadlineGuard {
        DeadlineGuard(DEADLINE.with(|d| d.replace(Some(t))))
    }
}

impl Drop for DeadlineGuard {
    fn drop(&mut self) {
        DEADLINE.with(|d| d.set(self.0));
    }
}

thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// A sub-agent: the same model and tools, a fresh conversation about one job, and only its final report comes back.
fn subagent(cfg: &Config, args: &Value, mode: Mode, approver: &dyn Approver, cancel: &AtomicBool) -> Result<String, String> {
    if DEPTH.with(|d| d.get()) > 0 {
        return Err("A sub-agent cannot start sub-agents.".into());
    }
    let task = args.get("task").and_then(Value::as_str).ok_or("missing argument: task")?;
    let sub_cfg = Config {
        provider: cfg.provider,
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        key: cfg.key.clone(),
        system: format!("{}\n\nYOU ARE A SUB-AGENT: another ZO-AI gave you one job and cannot see your steps. Do the job with your tools, then finish with a short, complete report of what you did and found (this report is all that comes back).", cfg.system),
        vision: cfg.vision,
    };
    let mut hist: Vec<Turn> = Vec::new();
    DEPTH.with(|d| d.set(1));
    let r = run(&sub_cfg, &mut hist, task, mode, approver, cancel, &mut |_| {});
    DEPTH.with(|d| d.set(0));
    r?;
    let report = hist.iter().rev().find_map(|t| match t { Turn::Assistant { text, .. } if !text.trim().is_empty() => Some(text.clone()), _ => None });
    Ok(report.unwrap_or_else(|| "The sub-agent finished without a report.".into()))
}

/// How many of the latest messages of yours (with their answers) stay as they are when the older conversation is summarized.
const KEEP_MESSAGES: usize = 2;

/// A text version of some turns, for the summary request. Long tool output is cut: its details are not what a summary needs.
fn transcript(turns: &[Turn]) -> String {
    let cut = |s: &str, n: usize| if s.chars().count() > n { format!("{}…", s.chars().take(n).collect::<String>()) } else { s.to_string() };
    turns
        .iter()
        .map(|t| match t {
            Turn::User(s) | Turn::UserImages(s, _) => format!("PERSON: {s}"),
            Turn::Assistant { text, calls } => {
                let tools = calls.iter().map(|c| format!("[used {} {}]", c.name, cut(&c.args.to_string(), 200))).collect::<Vec<_>>().join(" ");
                format!("ZO-AI: {text} {tools}").trim().to_string()
            }
            Turn::Results(rs) => rs.iter().map(|r| format!("[{} returned: {}]", r.name, cut(&r.content, 600))).collect::<Vec<_>>().join("\n"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replaces the older part of a conversation with a summary the model writes, keeping the last messages as they are.
/// Fails (and changes nothing) when the conversation is too short to be worth it.
pub fn compact(cfg: &Config, history: &mut Vec<Turn>, cancel: &AtomicBool, on_event: &mut dyn FnMut(Event)) -> Result<(), String> {
    let user_turns: Vec<usize> = history.iter().enumerate().filter(|(_, t)| matches!(t, Turn::User(_) | Turn::UserImages(..))).map(|(i, _)| i).collect();
    if user_turns.len() <= KEEP_MESSAGES {
        return Err("The conversation is too short to compact.".into());
    }
    let cut = user_turns[user_turns.len() - KEEP_MESSAGES];
    let older = &history[..cut];
    let prompt = format!(
        "Summarize the conversation below so that you can carry on from the summary alone. Keep what matters: what the person wants and has decided, the facts and names they gave, files, paths and commands that were used, what was done and what is still open. \
        Leave out small talk and long tool output. Write it as plain notes, as short as you can without losing those things.\n\n---\n{}",
        transcript(older)
    );
    let summariser = Config { provider: cfg.provider, base_url: cfg.base_url.clone(), model: cfg.model.clone(), key: cfg.key.clone(), system: "You write careful, compact summaries of conversations for an AI assistant that will continue them.".into(), vision: false };
    let reply = ask(&summariser, &[Turn::User(prompt)], &[], cancel, on_event)?;
    let summary = reply.text.trim().to_string();
    if summary.is_empty() {
        return Err("The model gave no summary.".into());
    }
    let mut next = vec![
        Turn::User(format!("[Summary of our earlier conversation, which was compacted to save space]\n{summary}")),
        Turn::Assistant { text: "Understood. I will carry on from that summary.".into(), calls: Vec::new() },
    ];
    next.extend(history.drain(cut..));
    *history = next;
    Ok(())
}

fn has_images(history: &[Turn]) -> bool {
    history.iter().any(|t| match t {
        Turn::UserImages(..) => true,
        Turn::Results(rs) => rs.iter().any(|r| !r.images.is_empty()),
        _ => false,
    })
}

/// Pictures become a short note in the text, so the conversation still makes sense without them.
pub(crate) fn strip_images(history: &mut [Turn]) {
    for t in history.iter_mut() {
        match t {
            Turn::UserImages(text, imgs) => {
                let note = format!("{text}\n[{} picture(s) left out: this model cannot read pictures]", imgs.len());
                *t = Turn::User(note);
            }
            Turn::Results(rs) => {
                for r in rs.iter_mut().filter(|r| !r.images.is_empty()) {
                    r.content.push_str(" [the picture was left out: this model cannot read pictures]");
                    r.images.clear();
                }
            }
            _ => {}
        }
    }
}

fn looks_like_picture_error(e: &str) -> bool {
    let e = e.to_lowercase();
    ["image", "vision", "multimodal", "multi-modal", "modalit", "image_url", "inlinedata"].iter().any(|k| e.contains(k))
}

/// The models a provider offers right now (its own list), or the built-in suggestions if it cannot be asked.
pub fn list_models(p: &Provider, base_url: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    if crate::tenant::is_restricted() {
        check_restricted_provider(p, base_url)?;
    }
    if p.protocol == Protocol::ClaudeCode {
        // Claude Code has no list of its own: with an Anthropic key the real list is read (and kept), else its aliases are offered
        let sel = Selection::load();
        if let (Some(k), Some(a)) = (super::profiles::get_key(&sel, "anthropic"), providers::find("anthropic")) {
            let _ = super::models::refresh(&sel.base_url_for(a), &k);
        }
        return Ok(super::models::claude_models().into_iter().map(|m| m.id).collect());
    }
    if p.id == "anthropic" {
        if let Some(k) = key {
            if let Ok(list) = super::models::refresh(base_url, k) {
                return Ok(list.into_iter().map(|m| m.id).collect());
            }
        }
    }
    let base = base_url.trim_end_matches('/');
    let mut r = agent().get(&format!("{base}/models"));
    match (p.protocol, key) {
        (Protocol::Anthropic, Some(k)) => r = r.set("x-api-key", k).set("anthropic-version", "2023-06-01"),
        (Protocol::Gemini, Some(k)) => r = r.set("x-goog-api-key", k),
        (_, Some(k)) => r = r.set("authorization", &format!("Bearer {k}")),
        _ => {}
    }
    let v: Value = r.call().map_err(http_error)?.into_json().map_err(|e| e.to_string())?;
    Ok(wire::parse_models(p.protocol, &v))
}

/// Which local model servers are answering on this computer right now.
pub fn detect_local() -> Vec<&'static Provider> {
    if crate::tenant::is_restricted() {
        return Vec::new(); // a server must not look around its own network
    }
    providers::all().iter().filter(|p| p.local && p.protocol != Protocol::ClaudeCode).filter(|p| {
        ureq::AgentBuilder::new().timeout(Duration::from_millis(400)).build().get(&format!("{}/models", p.base_url)).call().is_ok()
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::Deny;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A server that answers the given bodies in order and hands back every request body it saw.
    fn mock(answers: Vec<Value>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        serve(answers.into_iter().map(|a| ("application/json".to_string(), a.to_string())).collect())
    }

    fn serve(answers: Vec<(String, String)>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (ctype, body) in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        break (i + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
                seen.push(serde_json::from_slice(&buf[head_end..head_end + len]).unwrap());
                write!(s, "HTTP/1.1 200 OK\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            seen
        });
        (url, h)
    }

    /// Reads one whole HTTP request (headers and body) so the fake server never answers a half-sent one.
    fn read_request(s: &mut std::net::TcpStream) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = s.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                while buf.len() < i + 4 + len {
                    let n = s.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                return;
            }
        }
    }

    fn cfg(url: String, id: &str) -> Config<'static> {
        Config { provider: providers::find(id).unwrap(), base_url: url, model: "m".into(), key: Some("k".into()), system: "sys".into(), vision: false }
    }

    fn picture(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("zo-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(name);
        std::fs::write(&f, b"\x89PNG fake picture bytes").unwrap();
        f.to_string_lossy().into_owned()
    }

    #[test]
    fn an_old_conversation_is_summarized_and_the_last_messages_stay() {
        let (url, server) = mock(vec![json!({"choices":[{"message":{"content":"The person is setting up Spotify; the server is at example.com."}}]})]);
        let say = |t: &str| Turn::Assistant { text: t.into(), calls: Vec::new() };
        let mut history = vec![
            Turn::User("one".into()), say("a1"), Turn::User("two".into()), say("a2"), Turn::User("three".into()), say("a3"), Turn::User("four".into()), say("a4"),
        ];
        compact(&cfg(url, "openai"), &mut history, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let seen = server.join().unwrap();
        // the model was asked to summarize the first two messages and their answers, and nothing else
        let asked = seen[0]["messages"].to_string();
        assert!(asked.contains("PERSON: one") && asked.contains("ZO-AI: a2") && !asked.contains("three"), "{asked}");
        assert!(matches!(&history[0], Turn::User(t) if t.contains("Summary of our earlier conversation") && t.contains("Spotify")));
        assert!(matches!(&history[1], Turn::Assistant { .. }));
        assert_eq!(history.len(), 6);
        assert!(matches!(&history[2], Turn::User(t) if t == "three") && matches!(&history[4], Turn::User(t) if t == "four"));
        // a short conversation is left alone
        let mut short = vec![Turn::User("x".into()), say("y")];
        assert!(compact(&cfg("http://127.0.0.1:1/v1".into(), "openai"), &mut short, &AtomicBool::new(false), &mut |_| {}).is_err());
        assert_eq!(short.len(), 2);
    }

    #[test]
    fn a_long_conversation_is_compacted_before_the_next_question() {
        // a local-sized window (32k tokens) and a conversation that fills it: the first request is the summary, the second the answer
        let (url, server) = mock(vec![
            json!({"choices":[{"message":{"content":"Notes: they like blue."}}]}),
            json!({"choices":[{"message":{"content":"ok"}}]}),
        ]);
        let mut c = cfg(url, "ollama");
        c.model = "m".into();
        let big = "word ".repeat(30_000);
        let say = |t: &str| Turn::Assistant { text: t.into(), calls: Vec::new() };
        let mut history = vec![Turn::User(big.clone()), say("a"), Turn::User("two".into()), say("b"), Turn::User("three".into()), say("c")];
        let mut events = Vec::new();
        run(&c, &mut history, "four", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |e| if let Event::ToolDone { name, .. } = e { events.push(name) }).unwrap();
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0]["messages"].to_string().contains("Summarize the conversation"));
        assert!(!seen[1]["messages"].to_string().contains("word word word"), "the long old message is gone from the real request");
        assert!(events.contains(&"compaction".to_string()));
    }

    #[test]
    fn a_picture_the_model_asks_for_is_sent_to_a_model_that_can_see() {
        let png = picture("seen.png");
        let (url, server) = mock(vec![
            json!({"choices":[{"message":{"content":"Let me look.","tool_calls":[{"id":"c1","function":{"name":"zohara_view_image","arguments":json!({"path": png}).to_string()}}]}}]}),
            json!({"choices":[{"message":{"content":"A small test picture."}}]}),
        ]);
        let mut history = Vec::new();
        let mut c = cfg(url, "openai");
        c.vision = true;
        run(&c, &mut history, "what is in the picture?", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let seen = server.join().unwrap();
        let second = seen[1]["messages"].as_array().unwrap();
        let tool = second.iter().find(|m| m["role"] == "tool").unwrap();
        assert!(tool["content"].as_str().unwrap().contains("attached"));
        let pics = second.last().unwrap();
        assert_eq!(pics["role"], "user");
        assert!(pics["content"].as_array().unwrap().iter().any(|p| p["type"] == "image_url" && p["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,")));
        assert!(matches!(&history[2], Turn::Results(rs) if rs[0].images.len() == 1));
    }

    #[test]
    fn a_model_that_cannot_see_is_told_so_and_gets_no_picture() {
        let png = picture("unseen.png");
        let (url, server) = mock(vec![
            json!({"choices":[{"message":{"tool_calls":[{"id":"c1","function":{"name":"zohara_view_image","arguments":json!({"path": png}).to_string()}}]}}]}),
            json!({"choices":[{"message":{"content":"I cannot see it."}}]}),
        ]);
        let mut history = Vec::new();
        run(&cfg(url, "openai"), &mut history, "look", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let seen = server.join().unwrap();
        let text = seen[1].to_string();
        assert!(text.contains("cannot look at pictures") && !text.contains("image_url"));
    }

    #[test]
    fn a_picture_typed_into_the_message_goes_along_and_a_refusing_model_gets_it_left_out() {
        let png = picture("typed.png");
        // first the provider refuses pictures (as a text-only model does), then it answers
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for (status, body) in [(400, json!({"error":{"message":"This model does not support image input"}})), (200, json!({"choices":[{"message":{"content":"Ok, without it."}}]}))] {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                let (head_end, len) = loop {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        break (i + 4, head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0));
                    }
                };
                while buf.len() < head_end + len {
                    let n = s.read(&mut tmp).unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
                bodies.push(String::from_utf8_lossy(&buf[head_end..head_end + len]).into_owned());
                let body = body.to_string();
                write!(s, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            bodies
        });
        let mut c = cfg(url, "openrouter");
        c.vision = true;
        let mut history = Vec::new();
        run(&c, &mut history, &format!("what is this {png}?"), Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let bodies = server.join().unwrap();
        assert!(bodies[0].contains("image_url"), "the first request carries the picture");
        assert!(!bodies[1].contains("image_url") && bodies[1].contains("left out"), "the second is without it");
        assert!(matches!(&history[0], Turn::User(t) if t.contains("left out")));
    }

    #[test]
    fn the_loop_runs_a_tool_and_feeds_the_result_back() {
        let (url, server) = mock(vec![
            json!({"choices":[{"message":{"content":"Checking.","tool_calls":[{"id":"c1","function":{"name":"zohara_shell","arguments":"{\"cmd\":\"echo zo-loop-ok\"}"}}]}}]}),
            json!({"choices":[{"message":{"content":"It printed zo-loop-ok."}}]}),
        ]);
        let mut history = Vec::new();
        let mut events = Vec::new();
        run(&cfg(url, "openai"), &mut history, "say it", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |e| events.push(match e {
            Event::TextDelta(t) => format!("delta:{t}"),
            Event::Text(t) => format!("text:{t}"),
            Event::ToolStart { name, .. } => format!("start:{name}"),
            Event::ToolDone { ok, output, .. } => format!("done:{ok}:{output}"),
            Event::Usage(_) => "usage".to_string(),
        })).unwrap();
        assert_eq!(events, ["text:Checking.", "start:zohara_shell", "done:true:zo-loop-ok", "text:It printed zo-loop-ok."]);
        assert_eq!(history.len(), 4);
        let seen = server.join().unwrap();
        assert_eq!(seen[1]["messages"].as_array().unwrap().last().unwrap()["content"], "zo-loop-ok");
    }

    #[test]
    fn a_refused_tool_is_reported_to_the_model_not_run() {
        let (url, server) = mock(vec![
            json!({"content":[{"type":"tool_use","id":"u1","name":"zohara_fs_rm","input":{"path":"/tmp/zo-never-removed"}}]}),
            json!({"content":[{"type":"text","text":"Understood, I stopped."}]}),
        ]);
        let mut history = Vec::new();
        run(&cfg(url, "anthropic"), &mut history, "delete it", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let seen = server.join().unwrap();
        let result = &seen[1]["messages"].as_array().unwrap().last().unwrap()["content"][0];
        assert_eq!(result["is_error"], true);
        assert!(result["content"].as_str().unwrap().contains("did not approve"));
    }

    #[test]
    fn a_tool_from_an_added_mcp_server_is_offered_and_called() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (dir, server_cfg) = crate::mcp_client::fake_server();
        let conf = dir.join("config");
        std::fs::create_dir_all(conf.join("zo-ai")).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &conf);
        crate::mcp_client::save_servers(&[server_cfg]);
        let (url, server) = mock(vec![
            json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"x1","function":{"name":"my_fake__echo","arguments":"{\"text\":\"hi\"}"}}]}}]}),
            json!({"choices":[{"message":{"content":"It echoed."}}]}),
        ]);
        let mut events = Vec::new();
        run(&cfg(url, "openai"), &mut Vec::new(), "use the fake server", Mode::Bypass, &Deny, &AtomicBool::new(false), &mut |e| if let Event::ToolDone { output, .. } = e { events.push(output) }).unwrap();
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(events, ["echoed"]);
        let seen = server.join().unwrap();
        let offered: Vec<String> = seen[0]["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
        assert!(offered.contains(&"my_fake__echo".to_string()) && offered.contains(&"zohara_shell".to_string()));
        assert_eq!(seen[1]["messages"].as_array().unwrap().last().unwrap()["content"], "echoed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pressing_stop_ends_the_loop_before_asking_the_model() {
        let err = run(&cfg("http://127.0.0.1:9/v1".into(), "openai"), &mut Vec::new(), "hi", Mode::Auto, &Deny, &AtomicBool::new(true), &mut |_| {}).unwrap_err();
        assert_eq!(err, STOPPED);
    }

    #[test]
    fn a_streamed_answer_arrives_in_pieces_and_its_tool_call_is_assembled() {
        let sse = |lines: &[&str]| ("text/event-stream".to_string(), lines.iter().map(|l| format!("data: {l}\n\n")).collect::<String>());
        let (url, server) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"content":"Checking "}}]}"#,
                r#"{"choices":[{"delta":{"content":"now."}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"zohara_shell","arguments":"{\"cmd\":\"echo "}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"stream-ok\"}"}}]}}]}"#,
                "[DONE]",
            ]),
            sse(&[r#"{"choices":[{"delta":{"content":"It said "}}]}"#, r#"{"choices":[{"delta":{"content":"stream-ok."}}]}"#, "[DONE]"]),
        ]);
        let mut events = Vec::new();
        run(&cfg(url, "openai"), &mut Vec::new(), "go", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |e| events.push(match e {
            Event::TextDelta(t) => format!("delta:{t}"),
            Event::Text(t) => format!("text:{t}"),
            Event::ToolStart { name, .. } => format!("start:{name}"),
            Event::ToolDone { ok, output, .. } => format!("done:{ok}:{output}"),
            Event::Usage(_) => "usage".to_string(),
        })).unwrap();
        assert_eq!(events, ["delta:Checking ", "delta:now.", "text:Checking now.", "start:zohara_shell", "done:true:stream-ok", "delta:It said ", "delta:stream-ok.", "text:It said stream-ok."]);
        let seen = server.join().unwrap();
        assert_eq!(seen[0]["stream"], true);
    }

    #[test]
    fn stop_works_while_the_model_is_still_thinking() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut tmp = [0u8; 8192];
            let _ = s.read(&mut tmp);
            std::thread::sleep(Duration::from_secs(4));
        });
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            c2.store(true, Ordering::Relaxed);
        });
        let t0 = std::time::Instant::now();
        let err = run(&cfg(url, "openai"), &mut Vec::new(), "hi", Mode::Auto, &Deny, &cancel, &mut |_| {}).unwrap_err();
        assert_eq!(err, STOPPED);
        assert!(t0.elapsed() < Duration::from_secs(2), "stop took {:?}", t0.elapsed());
    }

    #[test]
    fn an_http_error_is_a_readable_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_request(&mut s);
            let body = r#"{"error":{"message":"invalid api key"}}"#;
            write!(s, "HTTP/1.1 401 Unauthorized\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let err = run(&cfg(url, "openai"), &mut Vec::new(), "hi", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert!(err.contains("401") && err.contains("invalid api key"), "{err}");
    }

    #[test]
    fn a_sub_agent_does_a_job_and_only_its_report_comes_back() {
        crate::term::enable_in_app();
        let (url, server) = mock(vec![
            // the main model hands a job over
            json!({"choices":[{"message":{"content":"","tool_calls":[{"id":"c1","type":"function","function":{"name":"zohara_ai_subagent","arguments":"{\"task\":\"count the moons of Mars\"}"}}]}}]}),
            // the sub-agent answers it
            json!({"choices":[{"message":{"content":"Mars has 2 moons."}}]}),
            // the main model finishes
            json!({"choices":[{"message":{"content":"Mars has two moons."}}]}),
        ]);
        let mut history = Vec::new();
        run(&cfg(url, "openai"), &mut history, "ask a helper", Mode::Auto, &Deny, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 3);
        // the sub-agent started fresh: it saw the job, not the main conversation, and could not start sub-agents
        let sub = seen[1].to_string();
        assert!(sub.contains("count the moons of Mars") && !sub.contains("ask a helper"), "{sub}");
        assert!(!seen[1]["tools"].to_string().contains("zohara_ai_subagent"));
        assert!(seen[0]["tools"].to_string().contains("zohara_ai_subagent"));
        // the main model got the report as the tool result
        assert!(seen[2]["messages"].to_string().contains("Mars has 2 moons."));
    }
}
