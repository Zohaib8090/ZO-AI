//! The tools: what ZO-AI (or any MCP client) can do to the machine. Names keep the old `zohara_*` form.

use crate::policy::Risk;
use serde_json::{json, Value};

mod android;
mod fs;
mod system;

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
        ToolDef { name: "zohara_waydroid_prop", description: "Read a Waydroid property.", risk: Read, params: &[("key", "string", "Property name", true)] },
        ToolDef { name: "zohara_waydroid_app_list", description: "Android apps installed in Waydroid.", risk: Change, params: &[] },
        ToolDef { name: "zohara_waydroid_app_install", description: "Install an APK into Waydroid.", risk: Change, params: &[("apk", "string", "Path to the APK", true)] },
        ToolDef { name: "zohara_waydroid_app_launch", description: "Start an Android app in Waydroid.", risk: Change, params: &[("package", "string", "Package name", true)] },
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
    match name {
        "zohara_fs_read" => fs::read(text(a, "path")?),
        "zohara_fs_write" => fs::write(text(a, "path")?, text(a, "content")?, flag(a, "append")),
        "zohara_fs_list" => fs::list(text(a, "path")?, flag(a, "recursive")),
        "zohara_fs_search" => fs::search(text(a, "pattern")?, a.get("path").and_then(Value::as_str).unwrap_or("~")),
        "zohara_fs_mkdir" => fs::mkdir(text(a, "path")?),
        "zohara_fs_rm" => fs::rm(text(a, "path")?, flag(a, "recursive")),
        "zohara_ss" => system::screenshot(),
        "zohara_system" => system::info(),
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
        "zohara_waydroid_status" => android::waydroid(&["status"], false),
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
