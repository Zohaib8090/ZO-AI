//! The tools that let the chat AI (or any MCP client) use the other parts of Zohara Chat: scheduled tasks, projects and saved chats.
//! What they change is what the app shows, because they share the same files.

use crate::store::{chats, projects, schedules};
use serde_json::Value;

fn text<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    a.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("missing argument: {key}"))
}

pub fn schedule(a: &Value) -> Result<String, String> {
    let when = schedules::parse_when(text(a, "when")?)?;
    let repeat = a.get("repeat").and_then(Value::as_str).unwrap_or("once");
    let task = schedules::add(a.get("title").and_then(Value::as_str).unwrap_or(""), text(a, "task")?, when, repeat, None)?;
    Ok(format!("Scheduled \"{}\" for {} ({}). It is on the Scheduled page, id {}.", task.title, schedules::format_when(task.next_run), task.repeat, task.id))
}

pub fn list_scheduled() -> Result<String, String> {
    let all = schedules::load();
    if all.is_empty() {
        return Ok("Nothing is scheduled.".into());
    }
    Ok(all.iter().map(|t| format!("{} | {} | next {} | {}{} | {}", t.id, t.title, schedules::format_when(t.next_run), t.repeat, if t.enabled { "" } else { " (off)" }, t.prompt)).collect::<Vec<_>>().join("\n"))
}

pub fn cancel_scheduled(a: &Value) -> Result<String, String> {
    let id = text(a, "id")?;
    if schedules::remove(id) { Ok(format!("Removed {id}.")) } else { Err(format!("No scheduled task has the id {id}.")) }
}

fn find_project<'a>(store: &'a mut projects::Store, key: &str) -> Option<&'a mut projects::Project> {
    let k = key.trim().to_lowercase();
    store.projects.iter_mut().find(|p| p.id == key || p.name.to_lowercase() == k)
}

pub fn list_projects() -> Result<String, String> {
    let s = projects::Store::load();
    if s.projects.is_empty() {
        return Ok("There are no projects.".into());
    }
    Ok(s.projects.iter().map(|p| format!("{} | {} | {} | {} memory notes", p.id, p.name, p.goal, p.memory.len())).collect::<Vec<_>>().join("\n"))
}

pub fn create_project(a: &Value) -> Result<String, String> {
    let mut s = projects::Store::load();
    let name = text(a, "name")?;
    if s.projects.iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
        return Err(format!("A project called \"{name}\" already exists."));
    }
    let id = s.add(name.to_string(), a.get("goal").and_then(Value::as_str).unwrap_or("").to_string());
    s.save();
    Ok(format!("Created the project \"{name}\" (id {id}). It is on the Projects page."))
}

pub fn add_memory(a: &Value) -> Result<String, String> {
    let mut s = projects::Store::load();
    let key = text(a, "project")?;
    let note = text(a, "text")?.to_string();
    let p = find_project(&mut s, key).ok_or_else(|| format!("There is no project \"{key}\"."))?;
    p.memory.push(projects::MemoryEntry { text: note, when: projects::relative(projects::now_millis()) });
    p.updated = projects::now_millis();
    let name = p.name.clone();
    s.save();
    Ok(format!("Added to the memory of \"{name}\"."))
}

pub fn search_chats(a: &Value) -> Result<String, String> {
    let hits = chats::search(text(a, "query")?, &chats::load());
    if hits.is_empty() {
        return Ok("No chat matches.".into());
    }
    Ok(hits.iter().take(15).map(|h| format!("{} | {} | {}", h.id, h.title, h.snippet)).collect::<Vec<_>>().join("\n"))
}

pub fn memory_search(a: &Value) -> Result<String, String> {
    use crate::store::memory;
    let store = memory::load();
    if !store.enabled {
        return Ok("Memory is switched off.".into());
    }
    let hits = memory::search(&store, text(a, "query")?, 8);
    if hits.is_empty() {
        return Ok("Nothing in memory matches.".into());
    }
    Ok(hits.iter().map(|h| if h.item.kind == "episode" { format!("[chat {}] \"{}\": {}", h.item.chat, h.item.title, h.item.text) } else { format!("({}) {}", h.item.kind, h.item.text) }).collect::<Vec<_>>().join("\n"))
}

pub fn memory_save(a: &Value) -> Result<String, String> {
    use crate::store::memory;
    if !memory::load().enabled {
        return Err("Memory is switched off, so nothing was saved.".into());
    }
    crate::tenant::check_room()?;
    let note = text(a, "text")?;
    let kind = a.get("kind").and_then(Value::as_str).unwrap_or("fact");
    match memory::remember(note, kind, "") {
        Some((_, true)) => Ok("Remembered.".into()),
        Some((_, false)) => Ok("Updated what I remembered about this.".into()),
        None => Err("That note is too short to keep.".into()),
    }
}

pub fn memory_forget(a: &Value) -> Result<String, String> {
    use crate::store::memory;
    let gone = memory::forget_matching(text(a, "what")?);
    if gone.is_empty() {
        return Ok("Nothing in memory matched that closely enough to forget. Use zohara_memory_search to see the notes.".into());
    }
    Ok(format!("Forgot: {}", gone.join(" | ")))
}

pub fn pin_chat(a: &Value) -> Result<String, String> {
    let key = text(a, "chat")?;
    let on = a.get("pinned").and_then(Value::as_bool).unwrap_or(true);
    let all = chats::load();
    let k = key.to_lowercase();
    let found = all.iter().find(|c| c.id == key).or_else(|| all.iter().find(|c| c.title.to_lowercase().contains(&k))).ok_or_else(|| format!("No chat matches \"{key}\"."))?;
    chats::set_pinned(&found.id, on);
    Ok(format!("{} \"{}\".", if on { "Pinned" } else { "Unpinned" }, found.title))
}

pub fn add_mcp_server(a: &Value) -> Result<String, String> {
    use crate::mcp_client::{self, ServerConfig};
    let name = mcp_client::clean_name(text(a, "name")?);
    let address = text(a, "address")?.trim().to_string();
    if name.is_empty() {
        return Err("The name has no letters or digits.".into());
    }
    if mcp_client::load_servers().iter().any(|s| s.name == name) {
        return Err(format!("A server named \"{name}\" is already added."));
    }
    let cfg = ServerConfig { name: name.clone(), command: address, enabled: true, ..Default::default() };
    let tools = mcp_client::connect(&cfg, a.get("token").and_then(Value::as_str), &mcp_client::open_in_browser)?;
    let mut all = mcp_client::load_servers();
    all.push(cfg);
    mcp_client::save_servers(&all);
    Ok(format!("Added \"{name}\". Its tools ({}) are yours from the next message on. It is listed in Zohara Chat > Settings > MCP servers.", tools.join(", ")))
}

pub fn add_mcp_config(config: &str) -> Result<String, String> {
    use crate::mcp_client;
    let found = mcp_client::import_json(config)?;
    let have: Vec<String> = mcp_client::load_servers().into_iter().map(|s| s.name).collect();
    let mut report = Vec::new();
    for cfg in found {
        if have.contains(&cfg.name) {
            report.push(format!("{}: already added", cfg.name));
            continue;
        }
        match mcp_client::connect(&cfg, None, &mcp_client::open_in_browser) {
            Ok(tools) => {
                let mut all = mcp_client::load_servers();
                all.push(cfg.clone());
                mcp_client::save_servers(&all);
                report.push(format!("{}: added, {} tools (yours from the next message on)", cfg.name, tools.len()));
            }
            Err(e) => report.push(format!("{}: not added: {e}", cfg.name)),
        }
    }
    Ok(report.join("\n"))
}

pub fn mcp_status() -> Result<String, String> {
    use crate::mcp_client;
    let all = mcp_client::load_servers();
    if all.is_empty() {
        return Ok("No MCP servers are added.".into());
    }
    let mut lines = Vec::new();
    for cfg in all {
        if !cfg.enabled {
            lines.push(format!("{}: switched off", cfg.name));
            continue;
        }
        lines.push(match mcp_client::test(&cfg) {
            Ok(tools) => format!("{}: works, {} tools (for example {})", cfg.name, tools.len(), tools.iter().take(6).cloned().collect::<Vec<_>>().join(", ")),
            Err(e) => format!("{}: not working: {e}", cfg.name),
        });
    }
    Ok(lines.join("\n"))
}

pub fn list_mcp_servers() -> Result<String, String> {
    let all = crate::mcp_client::load_servers();
    if all.is_empty() {
        return Ok("No MCP servers are added.".into());
    }
    Ok(all.iter().map(|s| format!("{} | {} | {}", s.name, s.command, if s.enabled { "on" } else { "off" })).collect::<Vec<_>>().join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_tools_change_what_the_app_shows() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-app-tools-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", &dir);

        let r = schedule(&json!({ "task": "tidy my downloads", "when": "now + 1 hour", "repeat": "daily" })).unwrap();
        assert!(r.contains("Scheduled") && r.contains("daily"));
        assert!(schedule(&json!({ "task": "x", "when": "never ever" })).is_err());
        let listed = list_scheduled().unwrap();
        assert!(listed.contains("tidy my downloads") && listed.contains("daily"));
        let id = schedules::load()[0].id.clone();
        assert!(cancel_scheduled(&json!({ "id": id })).unwrap().contains("Removed"));
        assert_eq!(list_scheduled().unwrap(), "Nothing is scheduled.");

        assert!(create_project(&json!({ "name": "Atlas", "goal": "map the thing" })).unwrap().contains("Created"));
        assert!(create_project(&json!({ "name": "atlas" })).is_err(), "no duplicates");
        assert!(add_memory(&json!({ "project": "atlas", "text": "uses metric units" })).unwrap().contains("Atlas"));
        assert!(add_memory(&json!({ "project": "nope", "text": "x" })).is_err());
        assert_eq!(projects::Store::load().projects[0].memory[0].text, "uses metric units");
        assert!(list_projects().unwrap().contains("Atlas"));

        chats::upsert(chats::Chat { id: "c1".into(), title: "disk space".into(), updated: 5, entries: vec![chats::Entry { kind: "msg-user".into(), text: "how full is my disk".into(), ..Default::default() }], ..Default::default() });
        assert!(search_chats(&json!({ "query": "disk" })).unwrap().contains("c1"));
        assert!(pin_chat(&json!({ "chat": "disk" })).unwrap().starts_with("Pinned"));
        assert!(chats::load()[0].pinned);
        assert!(pin_chat(&json!({ "chat": "disk", "pinned": false })).unwrap().starts_with("Unpinned"));
        assert!(pin_chat(&json!({ "chat": "zzz" })).is_err());

        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_DATA_HOME");
    }
}
