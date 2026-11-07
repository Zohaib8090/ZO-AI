//! The list of Claude models, read from where it really lives instead of being typed in here.
//!
//! * With an Anthropic API key, the Models API (`GET /v1/models`) gives every model with its name, release date and, for each, which
//!   `effort` levels it supports (`capabilities.effort`). The list is kept in a cache file and refreshed once a day.
//! * Without one, Claude Code's own aliases (`sonnet`, `opus`, `haiku`: always the newest of each) are offered, plus any extra models
//!   Claude Code has written into its own cache.
//! * What Claude Desktop shows comes from Claude's account service, which a program cannot ask for you.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// When it was released (an ISO date as the API gives it); newest first is how lists are ordered.
    #[serde(default)]
    pub created: String,
    /// The effort levels it supports, when known (`None`: the API did not say).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efforts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The size of its window in tokens, when the API said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
}

/// The effort levels a model can have, lowest first (the names the API and Claude Code use).
pub const EFFORT_ORDER: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

fn cache_file() -> std::path::PathBuf {
    crate::audit::data_dir().join("claude-models.json")
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    fetched: u64,
    models: Vec<ModelInfo>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load_cache() -> Cache {
    std::fs::read_to_string(cache_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_cache(models: &[ModelInfo]) {
    let _ = std::fs::create_dir_all(crate::audit::data_dir());
    if let Ok(s) = serde_json::to_string(&Cache { fetched: now(), models: models.to_vec() }) {
        let _ = std::fs::write(cache_file(), s);
    }
}

/// "claude-opus-4-8" -> "Opus 4.8"; "claude-fable-5-1[1m]" -> "Fable 5.1". Used when no name was given.
pub fn pretty_name(id: &str) -> String {
    let id = id.split('[').next().unwrap_or(id);
    let id = id.strip_prefix("claude-").unwrap_or(id);
    let mut family = String::new();
    let mut numbers: Vec<&str> = Vec::new();
    for part in id.split('-') {
        if part.chars().all(|c| c.is_ascii_digit()) && !part.is_empty() {
            // a date suffix such as 20251101 is not a version
            if part.len() <= 2 {
                numbers.push(part);
            }
        } else if numbers.is_empty() {
            if !family.is_empty() {
                family.push(' ');
            }
            family.push_str(part);
        }
    }
    let family: String = family.split(' ').map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" ");
    if numbers.is_empty() { family } else { format!("{family} {}", numbers.join(".")) }
}

fn parse_item(m: &Value) -> Option<ModelInfo> {
    let id = m["id"].as_str()?.to_string();
    let name = m["display_name"].as_str().map(|n| n.strip_prefix("Claude ").unwrap_or(n).to_string()).unwrap_or_else(|| pretty_name(&id));
    let efforts = m["capabilities"]["effort"].as_object().filter(|e| e.get("supported") == Some(&Value::Bool(true))).map(|e| {
        EFFORT_ORDER.iter().filter(|l| e.get(**l).is_some_and(|v| v["supported"] == true)).map(|l| l.to_string()).collect::<Vec<_>>()
    });
    let efforts = match (m["capabilities"]["effort"].is_object(), efforts) {
        (true, Some(v)) => Some(v),
        (true, None) => Some(Vec::new()), // the model has no effort setting
        _ => None,
    };
    Some(ModelInfo { id, name, created: m["created_at"].as_str().unwrap_or("").to_string(), efforts, note: String::new(), max_input_tokens: m["max_input_tokens"].as_u64() })
}

/// Every model the key can see, newest first. Follows the pages of the list.
pub fn fetch_anthropic(base_url: &str, key: &str) -> Result<Vec<ModelInfo>, String> {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout(Duration::from_secs(20)).build();
    let base = base_url.trim_end_matches('/');
    let mut out: Vec<ModelInfo> = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..10 {
        let mut req = agent.get(&format!("{base}/models")).query("limit", "100").set("x-api-key", key).set("anthropic-version", "2023-06-01");
        if let Some(a) = &after {
            req = req.query("after_id", a);
        }
        let v: Value = req.call().map_err(|e| format!("The model list could not be loaded: {e}"))?.into_json().map_err(|e| e.to_string())?;
        out.extend(v["data"].as_array().into_iter().flatten().filter_map(parse_item));
        match (v["has_more"].as_bool(), v["last_id"].as_str()) {
            (Some(true), Some(l)) => after = Some(l.to_string()),
            _ => break,
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(out)
}

/// Loads the list with the Anthropic key (if there is one) and keeps it. Returns what was loaded.
pub fn refresh(base_url: &str, key: &str) -> Result<Vec<ModelInfo>, String> {
    let list = fetch_anthropic(base_url, key)?;
    save_cache(&list);
    Ok(list)
}

/// Refreshes the cache when it is older than a day (does nothing without a key). For a background thread at start.
pub fn refresh_if_stale(base_url: &str, key: Option<&str>) {
    let c = load_cache();
    if let Some(k) = key {
        if c.models.is_empty() || now().saturating_sub(c.fetched) > 24 * 3600 {
            let _ = refresh(base_url, k);
        }
    }
}

/// What Claude Code wrote in its own cache about extra models (`~/.claude.json`, only that one entry; nothing about the login is read).
fn claude_code_extras() -> Vec<ModelInfo> {
    let path = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".claude.json"));
    let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    v["additionalModelOptionsCache"].as_array().into_iter().flatten().filter_map(|m| {
        let id = m["value"].as_str()?.split('[').next()?.to_string();
        Some(ModelInfo { name: pretty_name(&id), id, note: m["description"].as_str().unwrap_or("").to_string(), ..Default::default() })
    }).collect()
}

/// The models to offer: the cached Models API list; else Claude Code's aliases and its own extras.
pub fn claude_models() -> Vec<ModelInfo> {
    let cached = load_cache().models;
    if !cached.is_empty() {
        return cached;
    }
    let mut v: Vec<ModelInfo> = ["opus", "sonnet", "haiku"].iter().map(|a| ModelInfo { id: a.to_string(), name: format!("{} (latest)", pretty_name(a)), note: "Always the newest of its kind".into(), ..Default::default() }).collect();
    for e in claude_code_extras() {
        if !v.iter().any(|m| m.id == e.id) {
            v.push(e);
        }
    }
    v
}

/// How a model is named in the interface: the name the API gave, else made from its id.
pub fn display_name(id: &str) -> String {
    claude_models().into_iter().find(|m| m.id == id).map(|m| m.name).unwrap_or_else(|| pretty_name(id))
}

/// The effort levels to offer for a model: what the API said, or all five when it did not say; none when the model has no effort setting.
pub fn effort_levels(id: &str) -> Vec<String> {
    match claude_models().into_iter().find(|m| m.id == id).and_then(|m| m.efforts) {
        Some(levels) => levels,
        None => EFFORT_ORDER.iter().map(|l| l.to_string()).collect(),
    }
}

/// The family of a model, for choosing the newest of each: opus, sonnet, haiku, fable, or the id itself.
pub fn family(id: &str) -> String {
    let id = id.strip_prefix("claude-").unwrap_or(id);
    id.split('-').next().unwrap_or(id).to_string()
}

/// (main, more): the newest model of each family first, then all the others, each group newest first.
pub fn split_main(models: &[ModelInfo]) -> (Vec<ModelInfo>, Vec<ModelInfo>) {
    let mut seen: Vec<String> = Vec::new();
    let (mut main, mut more) = (Vec::new(), Vec::new());
    for m in models {
        let f = family(&m.id);
        if !seen.contains(&f) {
            seen.push(f);
            main.push(m.clone());
        } else {
            more.push(m.clone());
        }
    }
    (main, more)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_are_made_from_ids_and_the_api_item_is_read() {
        assert_eq!(pretty_name("claude-opus-4-8"), "Opus 4.8");
        assert_eq!(pretty_name("claude-fable-5-1[1m]"), "Fable 5.1");
        assert_eq!(pretty_name("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(pretty_name("sonnet"), "Sonnet");
        let m = parse_item(&json!({ "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "created_at": "2026-09-01T00:00:00Z",
            "capabilities": { "effort": { "supported": true, "low": { "supported": true }, "medium": { "supported": true }, "high": { "supported": true }, "xhigh": { "supported": false }, "max": { "supported": true } } } })).unwrap();
        assert_eq!((m.name.as_str(), m.efforts.clone().unwrap()), ("Opus 5.5", vec!["low".to_string(), "medium".into(), "high".into(), "max".into()]));
        let none = parse_item(&json!({ "id": "claude-haiku-4-5", "capabilities": { "effort": { "supported": false } } })).unwrap();
        assert_eq!(none.efforts, Some(vec![]));
        assert_eq!(parse_item(&json!({ "id": "x" })).unwrap().efforts, None);
    }

    #[test]
    fn the_newest_of_each_family_comes_first() {
        let m = |id: &str| ModelInfo { id: id.into(), name: pretty_name(id), ..Default::default() };
        let all = vec![m("claude-opus-5-5"), m("claude-sonnet-5-5"), m("claude-opus-5"), m("claude-haiku-5-5"), m("claude-opus-4-8")];
        let (main, more) = split_main(&all);
        assert_eq!(main.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"]);
        assert_eq!(more.len(), 2);
    }
}
