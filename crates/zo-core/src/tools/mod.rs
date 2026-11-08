//! The tools: what ZO-AI (or any MCP client) can do to the machine. Names keep the old `zohara_*` form.

use crate::policy::Risk;
use serde_json::{json, Value};

mod android;
mod app;
mod fs;
mod system;
mod terminals;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub risk: Risk,
    /// (name, json type, description, required)
    pub params: &'static [(&'static str, &'static str, &'static str, bool)],
}

impl ToolDef {
    pub fn schema(&self) -> Value {
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        for (n, t, d, req) in self.params {
            props.insert((*n).into(), json!({ "type": t, "description": d }));
            if *req {
                required.push(json!(n));
            }
        }
        json!({ "type": "object", "properties": props, "required": required })
    }
}

const P_PATH: (&str, &str, &str, bool) = ("path", "string", "File or folder path (~ is your home)", true);

/// Tools that need the ZO-AI window (terminals, the browser, sub-agents). Offered only inside the app.
pub fn app_definitions() -> &'static [ToolDef] {
    use Risk::*;
    const ID: (&str, &str, &str, bool) = ("id", "integer", "The terminal id (zohara_term_list)", true);
    &[
        ToolDef { name: "zohara_term_open", description: "Open a new terminal in the ZO-AI window (a real shell the person can see and type in). Use several terminals for several jobs, for example a server in one and tests in another.", risk: Change,
            params: &[("name", "string", "A short name for the tab", false), ("cwd", "string", "Folder to start in", false)] },
        ToolDef { name: "zohara_term_run", description: "Type a command into a terminal, wait until its output settles, and return what the terminal shows. Without an id it uses your last terminal, or opens one. Long-running things (servers, watchers) keep running in the terminal: read them later with zohara_term_read.", risk: Shell,
            params: &[("cmd", "string", "The command", true), ("id", "integer", "Terminal id", false), ("timeout", "integer", "Seconds to wait for output, default 30", false)] },
        ToolDef { name: "zohara_term_send", description: "Type raw text or press a key in a terminal, for programs that ask questions or take over the screen (editors, installers, a running process: Ctrl+C stops it).", risk: Change,
            params: &[ID, ("text", "string", "Text to type (no Enter added)", false), ("key", "string", "A key: Enter, Tab, Escape, Up, Down, Left, Right, Backspace, Ctrl+C, Ctrl+D …", false)] },
        ToolDef { name: "zohara_term_read", description: "Read the latest output of a terminal, including what the person typed there.", risk: Read,
            params: &[ID, ("lines", "integer", "How many lines, default 60", false)] },
        ToolDef { name: "zohara_term_list", description: "List the terminals that are open in the ZO-AI window.", risk: Read, params: &[] },
        ToolDef { name: "zohara_term_close", description: "Close a terminal (stops what runs in it).", risk: Change, params: &[ID] },
        ToolDef { name: "zohara_browser_open", description: "Open a web page in the browser built into the ZO-AI window (a full browser the person can see and use too) and wait for it to load. Pass a full address, or words to search the web.", risk: Read,
            params: &[("url", "string", "Address (https://…) or search words", true)] },
        ToolDef { name: "zohara_browser_read", description: "Read the page now open in the built-in browser: its address, title, the visible text and the links and buttons you can use (each with a number for zohara_browser_click).", risk: Read, params: &[] },
        ToolDef { name: "zohara_browser_click", description: "Click something on the page: a link or button by its number from zohara_browser_read, or by a CSS selector, or by the text it shows.", risk: Change,
            params: &[("target", "string", "A number from zohara_browser_read, a CSS selector (starting with # or . or [), or the visible text", true)] },
        ToolDef { name: "zohara_browser_type", description: "Type into a text field on the page (a number from zohara_browser_read, or a CSS selector), and press Enter when submit is true.", risk: Change,
            params: &[("target", "string", "A number from zohara_browser_read or a CSS selector", true), ("text", "string", "The text to type", true), ("submit", "boolean", "Press Enter afterwards", false)] },
        ToolDef { name: "zohara_browser_scroll", description: "Scroll the page (down, up, top or bottom), then read it again to see the new part.", risk: Read,
            params: &[("where", "string", "down, up, top or bottom", true)] },
        ToolDef { name: "zohara_browser_back", description: "Go back one page in the built-in browser (or forward when forward is true).", risk: Read,
            params: &[("forward", "boolean", "Go forward instead", false)] },
        ToolDef { name: "zohara_browser_screenshot", description: "Take a picture of the page in the built-in browser; returns the PNG path (shown to you when the model can see pictures).", risk: Read, params: &[] },
        ToolDef { name: "zohara_browser_js", description: "Run JavaScript on the page and return its result. For things the other browser tools cannot do.", risk: Change,
            params: &[("script", "string", "JavaScript; the value of the last expression is returned", true)] },
        ToolDef { name: "zohara_ai_subagent", description: "Hand a self-contained job to a sub-agent: a second ZO-AI with the same tools that works on it alone and returns its final report. Use it for a side investigation or a long chore so this conversation stays short; call it several times for separate jobs. A sub-agent cannot start sub-agents. Give it everything it needs: it does not see this conversation.", risk: Read,
            params: &[("task", "string", "The full job, written as instructions, with all the details it needs", true), ("name", "string", "A short name for the job", false)] },
    ]
}

/// The tools of this process: the ones every client gets, plus the app's own when running inside the app.
pub fn active() -> Vec<&'static ToolDef> {
    let mut v: Vec<&'static ToolDef> = definitions().iter().collect();
    if crate::term::enabled() {
        // terminals work without a window; the browser needs the window that shows it
        v.extend(app_definitions().iter().filter(|d| !d.name.starts_with("zohara_browser_") || crate::uibridge::available()));
    }
    v
}

pub fn find(name: &str) -> Option<&'static ToolDef> {
    active().into_iter().find(|d| d.name == name)
}

pub fn definitions() -> &'static [ToolDef] {
    use Risk::*;
    &[
        ToolDef { name: "zohara_fs_read", description: "Read a text file (up to 1 MB).", risk: Read, params: &[P_PATH] },
        ToolDef { name: "zohara_fs_write", description: "Write a text file, or append to it.", risk: Change,
            params: &[P_PATH, ("content", "string", "Text to write", true), ("append", "boolean", "Add to the end instead of replacing", false)] },
        ToolDef { name: "zohara_fs_list", description: "List a folder.", risk: Read,
            params: &[P_PATH, ("recursive", "boolean", "Go into sub-folders (3 levels, 500 entries)", false)] },
        ToolDef { name: "zohara_fs_search", description: "Find files by name (case-insensitive, * allowed).", risk: Read,
            params: &[("pattern", "string", "Name or pattern, e.g. *.png", true), ("path", "string", "Where to look (default: home)", false)] },
        ToolDef { name: "zohara_fs_mkdir", description: "Create a folder (and its parents).", risk: Change, params: &[P_PATH] },
        ToolDef { name: "zohara_fs_rm", description: "Delete a file, or a folder with recursive=true. Never the home folder or /.", risk: Change,
            params: &[P_PATH, ("recursive", "boolean", "Delete a folder and everything in it", false)] },
        ToolDef { name: "zohara_ss", description: "Take a screenshot of the desktop; returns the PNG path.", risk: Read, params: &[] },
        ToolDef { name: "zohara_view_image", description: "Look at a picture file (png, jpg, webp or gif): it is shown to you with the result. Use it for a screenshot you were told about, or any image the person points to. Only models that can see pictures get it.", risk: Read,
            params: &[("path", "string", "Path of the picture file", true)] },
        ToolDef { name: "zohara_system", description: "CPU, memory, GPU, disks, kernel, user.", risk: Read, params: &[] },
        ToolDef { name: "zohara_shell", description: "Run a shell command (2 minutes at most).", risk: Shell,
            params: &[("cmd", "string", "The command", true), ("timeout", "integer", "Seconds, default 120", false)] },
        ToolDef { name: "zohara_process_list", description: "Running processes, busiest first (top 40).", risk: Read, params: &[] },
        ToolDef { name: "zohara_process_kill", description: "Stop a process by id (SIGTERM, or SIGKILL with force).", risk: Change,
            params: &[("pid", "integer", "Process id (2 or higher)", true), ("force", "boolean", "SIGKILL", false)] },
        ToolDef { name: "zohara_zohara_status", description: "Zohara OS snapshot status.", risk: Read, params: &[] },
        ToolDef { name: "zohara_zohara_list", description: "List Zohara OS snapshots.", risk: Read, params: &[] },
        ToolDef { name: "zohara_adb_devices", description: "List connected Android devices.", risk: Read, params: &[] },
        ToolDef { name: "zohara_adb_shell", description: "Run a command on the Android device.", risk: Change,
            params: &[("commands", "string", "Command line to run on the device", true)] },
        ToolDef { name: "zohara_adb_install", description: "Install an APK on the device.", risk: Change, params: &[("apk", "string", "Path to the APK", true)] },
        ToolDef { name: "zohara_adb_uninstall", description: "Remove an app from the device.", risk: Change, params: &[("package", "string", "Package name", true)] },
        ToolDef { name: "zohara_adb_push", description: "Copy a file to the device.", risk: Change,
            params: &[("local", "string", "Local path", true), ("remote", "string", "Path on the device", true)] },
        ToolDef { name: "zohara_adb_pull", description: "Copy a file from the device.", risk: Change,
            params: &[("remote", "string", "Path on the device", true), ("local", "string", "Local path", true)] },
        ToolDef { name: "zohara_waydroid_status", description: "Waydroid (Android on Zohara OS) status.", risk: Read, params: &[] },
        ToolDef { name: "zohara_waydroid_start", description: "Start Waydroid (Android on Zohara OS) when its session is stopped, and open its window. Do this before listing or launching Android apps.", risk: Change, params: &[] },
        ToolDef { name: "zohara_waydroid_prop", description: "Read a Waydroid property.", risk: Read, params: &[("key", "string", "Property name", true)] },
        ToolDef { name: "zohara_waydroid_app_list", description: "Android apps installed in Waydroid.", risk: Change, params: &[] },
        ToolDef { name: "zohara_waydroid_app_install", description: "Install an APK into Waydroid.", risk: Change, params: &[("apk", "string", "Path to the APK", true)] },
        ToolDef { name: "zohara_waydroid_app_launch", description: "Start an Android app in Waydroid.", risk: Change, params: &[("package", "string", "Package name", true)] },
        ToolDef { name: "zohara_ai_schedule", description: "Schedule a task for ZO-AI to do later, once or on a repeat. It appears on the Scheduled page.", risk: Change,
            params: &[("task", "string", "What ZO-AI should do when the time comes, written as an instruction", true), ("when", "string", "When, in plain words: \"tomorrow 9am\", \"in 2 hours\", \"2026-11-05 18:30\"", true), ("repeat", "string", "once, hourly, daily, weekly or monthly (default once), or your own interval such as \"every 3 days\", \"every 90 minutes\" or \"every 2 months\" (at most every 5 minutes)", false), ("title", "string", "A short name for the list", false)] },
        ToolDef { name: "zohara_ai_list_scheduled", description: "List the scheduled tasks and when they run next.", risk: Read, params: &[] },
        ToolDef { name: "zohara_ai_cancel_scheduled", description: "Remove a scheduled task.", risk: Change, params: &[("id", "string", "The task id from the list", true)] },
        ToolDef { name: "zohara_ai_projects", description: "List the projects (name, goal, memory notes).", risk: Read, params: &[] },
        ToolDef { name: "zohara_ai_create_project", description: "Create a project. It appears on the Projects page.", risk: Change,
            params: &[("name", "string", "The project name", true), ("goal", "string", "What the project is for", false)] },
        ToolDef { name: "zohara_ai_add_memory", description: "Add a note to a project's memory, so ZO-AI remembers it in that project.", risk: Change,
            params: &[("project", "string", "Project name or id", true), ("text", "string", "The note", true)] },
        ToolDef { name: "zohara_ai_add_mcp_server", description: "Add an MCP server to ZO-AI itself, so its tools become your own tools from the next message on. Use this when the person gives you an MCP server (an https address, or a program that runs one). It is tried once before it is saved; if it asks for a sign-in, the person's browser opens and they sign in.", risk: Change,
            params: &[("name", "string", "A short name, e.g. spotify", true), ("address", "string", "The https address of a remote server, or the command that starts a local one (e.g. `python3 /home/me/server/app.py`)", true), ("token", "string", "An access token, only if the server gave one", false)] },
        ToolDef { name: "zohara_ai_add_mcp_config", description: "Add MCP servers to ZO-AI itself from a JSON configuration, the kind a server's README gives (a \"mcpServers\" object with a command and args, or a url and headers; Claude Desktop's format works as is). Every server in it is tried, signed in to if it asks, and added if it works.", risk: Change,
            params: &[("config", "string", "The JSON text", true)] },
        ToolDef { name: "zohara_ai_mcp_status", description: "Connect to each MCP server that is added to ZO-AI and say whether it works: how many tools it has (and the first few names), or what is wrong (a sign-in is needed, it is not reachable). Use it when a server's tools are missing.", risk: Read, params: &[] },
        ToolDef { name: "zohara_ai_list_mcp_servers", description: "List the MCP servers that are added to ZO-AI.", risk: Read, params: &[] },
        ToolDef { name: "zohara_ai_search_chats", description: "Search the saved chats.", risk: Read, params: &[("query", "string", "Words to look for", true)] },
        ToolDef { name: "zohara_ai_pin_chat", description: "Pin or unpin a saved chat in the sidebar.", risk: Change,
            params: &[("chat", "string", "Chat id or part of its title", true), ("pinned", "boolean", "true to pin (default), false to unpin", false)] },
        ToolDef { name: "zohara_waydroid_shell", description: "Run a command inside Waydroid (asks for your password).", risk: Change,
            params: &[("cmds", "string", "Command line", true)] },
    ]
}

pub(crate) fn text<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    a.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing argument: {key}"))
}

pub(crate) fn flag(a: &Value, key: &str) -> bool {
    a.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(crate) fn expand(p: &str) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    match p {
        "~" => home.into(),
        _ if p.starts_with("~/") => std::path::Path::new(&home).join(&p[2..]),
        _ => p.into(),
    }
}

/// Run a tool. The caller has already decided it is allowed (see `policy`).
pub fn call(name: &str, a: &Value) -> Result<String, String> {
    let result = call_inner(name, a);
    // what it made is kept under Artifacts
    if result.is_ok() {
        match name {
            "zohara_fs_write" => {
                if let Some(p) = a.get("path").and_then(Value::as_str) {
                    crate::store::artifacts::add_file(p, "file");
                }
            }
            "zohara_ss" => {
                if let Ok(p) = &result {
                    crate::store::artifacts::add_file(p, "screenshot");
                }
            }
            _ => {}
        }
    }
    result
}

fn call_inner(name: &str, a: &Value) -> Result<String, String> {
    match name {
        "zohara_fs_read" => fs::read(text(a, "path")?),
        "zohara_fs_write" => fs::write(text(a, "path")?, text(a, "content")?, flag(a, "append")),
        "zohara_fs_list" => fs::list(text(a, "path")?, flag(a, "recursive")),
        "zohara_fs_search" => fs::search(text(a, "pattern")?, a.get("path").and_then(Value::as_str).unwrap_or("~")),
        "zohara_fs_mkdir" => fs::mkdir(text(a, "path")?),
        "zohara_fs_rm" => fs::rm(text(a, "path")?, flag(a, "recursive")),
        "zohara_ss" => system::screenshot(),
        "zohara_system" => system::info(),
        "zohara_view_image" => system::view_image(text(a, "path")?),
        "zohara_shell" => system::shell(text(a, "cmd")?, a.get("timeout").and_then(Value::as_u64).unwrap_or(120)),
        "zohara_process_list" => system::processes(),
        "zohara_process_kill" => system::kill(a.get("pid").and_then(Value::as_u64).ok_or("missing argument: pid")?, flag(a, "force")),
        "zohara_zohara_status" => crate::exec::simple("zohara-snapshots", &["list-json"]),
        "zohara_zohara_list" => crate::exec::simple("zohara-snapshots", &["list"]),
        "zohara_adb_devices" => android::adb(&["devices"]),
        "zohara_adb_shell" => android::adb_shell(text(a, "commands")?),
        "zohara_adb_install" => android::adb(&["install", "-r", text(a, "apk")?]),
        "zohara_adb_uninstall" => android::adb(&["uninstall", text(a, "package")?]),
        "zohara_adb_push" => android::adb(&["push", text(a, "local")?, text(a, "remote")?]),
        "zohara_adb_pull" => android::adb(&["pull", text(a, "remote")?, text(a, "local")?]),
        "zohara_ai_schedule" => app::schedule(a),
        "zohara_ai_list_scheduled" => app::list_scheduled(),
        "zohara_ai_cancel_scheduled" => app::cancel_scheduled(a),
        "zohara_ai_projects" => app::list_projects(),
        "zohara_ai_create_project" => app::create_project(a),
        "zohara_ai_add_memory" => app::add_memory(a),
        "zohara_ai_search_chats" => app::search_chats(a),
        "zohara_ai_pin_chat" => app::pin_chat(a),
        "zohara_ai_add_mcp_server" => app::add_mcp_server(a),
        "zohara_ai_list_mcp_servers" => app::list_mcp_servers(),
        "zohara_ai_mcp_status" => app::mcp_status(),
        "zohara_ai_add_mcp_config" => app::add_mcp_config(text(a, "config")?),
        "zohara_term_open" if crate::term::enabled() => terminals::open(a),
        "zohara_term_run" if crate::term::enabled() => terminals::run(a),
        "zohara_term_send" if crate::term::enabled() => terminals::send(a),
        "zohara_term_read" if crate::term::enabled() => terminals::read(a),
        "zohara_term_list" if crate::term::enabled() => terminals::list(),
        "zohara_term_close" if crate::term::enabled() => terminals::close(a),
        n if n.starts_with("zohara_browser_") && crate::term::enabled() => crate::uibridge::call(n, a),
        "zohara_waydroid_status" => android::waydroid(&["status"], false),
        "zohara_waydroid_start" => android::waydroid_start(),
        "zohara_waydroid_prop" => android::waydroid(&["prop", "get", text(a, "key")?], false),
        "zohara_waydroid_app_list" => android::waydroid(&["app", "list"], true),
        "zohara_waydroid_app_install" => android::waydroid(&["app", "install", text(a, "apk")?], true),
        "zohara_waydroid_app_launch" => android::waydroid(&["app", "launch", text(a, "package")?], true),
        "zohara_waydroid_shell" => {
            let mut args = vec!["shell", "--"];
            args.extend(text(a, "cmds")?.split_whitespace());
            android::waydroid(&args, true)
        }
        other => Err(format!("unknown tool: {other}")),
    }
}
