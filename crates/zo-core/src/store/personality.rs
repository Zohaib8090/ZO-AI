//! Personalities: how ZO-AI talks. A personality is a short piece of instructions about tone and manner that is added to the
//! system prompt. It changes the voice, never the rules: honesty, permissions and the person's instructions stay as they are.
//! The person can pick one, write one, or ask the AI to write one from a description (`ai::personality_gen`).
//! One file (`personalities.json`) for the window and the phone.

use super::chats;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Personality {
    pub id: String,
    pub name: String,
    /// The instructions: how to sound.
    pub prompt: String,
    /// Made by the AI from a description (shown so the person can tell).
    #[serde(default)]
    pub generated: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Store {
    /// The one in use; empty means ZO-AI's own plain manner.
    #[serde(default)]
    pub active: String,
    #[serde(default)]
    pub items: Vec<Personality>,
}

static LOCK: Mutex<()> = Mutex::new(());

fn path() -> PathBuf {
    crate::audit::config_dir().join("personalities.json")
}

/// The built-in ones: always there, not stored, cannot be deleted.
pub fn builtin() -> Vec<Personality> {
    let b = |id: &str, name: &str, prompt: &str| Personality { id: id.into(), name: name.into(), prompt: prompt.into(), generated: false };
    vec![
        b("friend", "Friendly buddy", "Sound like a warm, relaxed friend who is good with computers. Casual and kind, a little humour when it fits, never sarcastic at the person's expense. Short sentences. Celebrate small wins. If they sound stressed or stuck, say one reassuring thing, then help."),
        b("engineer", "Straight engineer", "Sound like a senior engineer who respects the person's time. Direct and precise: the answer first, then only the reasons that matter. No small talk, no filler, no apologies. Say plainly when something is a bad idea and what to do instead."),
        b("mentor", "Calm mentor", "Sound like a patient mentor. Explain the why in a sentence when it helps them learn, check understanding with one short question when a step is easy to get wrong, and encourage them to try the next step themselves. Calm, never condescending."),
        b("sharp", "Dry and witty", "Sound dry and quick-witted: understated humour, light teasing at most, never at the cost of being useful. The jokes are one line and the help comes first."),
    ]
}

pub fn load() -> Store {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn change<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load();
    let r = f(&mut s);
    let p = path();
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
    let tmp = p.with_extension("json.tmp");
    if let Ok(t) = serde_json::to_string_pretty(&s) {
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
    r
}

/// Built-in ones first, then the person's own.
pub fn all() -> Vec<Personality> {
    let mut v = builtin();
    v.extend(load().items);
    v
}

pub fn active() -> Option<Personality> {
    let id = load().active;
    if id.is_empty() {
        return None;
    }
    all().into_iter().find(|p| p.id == id)
}

pub fn set_active(id: &str) -> Result<(), String> {
    if !id.is_empty() && !all().iter().any(|p| p.id == id) {
        return Err("There is no such personality.".into());
    }
    change(|s| s.active = id.to_string());
    Ok(())
}

pub fn add(name: &str, prompt: &str, generated: bool) -> Result<Personality, String> {
    let name: String = name.trim().chars().take(40).collect();
    let prompt: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(1200).collect();
    if name.is_empty() || prompt.chars().count() < 10 {
        return Err("A personality needs a name and a few sentences about how to sound.".into());
    }
    let p = Personality { id: format!("p{}", chats::now_millis()), name, prompt, generated };
    change(|s| s.items.push(p.clone()));
    Ok(p)
}

pub fn remove(id: &str) {
    change(|s| {
        s.items.retain(|p| p.id != id);
        if s.active == id {
            s.active.clear();
        }
    });
}

/// What is added to the system prompt (empty when no personality is in use).
pub fn instructions() -> String {
    match active() {
        Some(p) => format!(
            "PERSONALITY: {}\nThis is how you sound with this person: {}\n\
            It changes your voice only. Stay honest, keep to the permissions and the person's instructions, and do not invent facts to fit the character. \
            Never let the personality get in the way of a clear answer or a safe action.",
            p.name, p.prompt
        ),
        None => String::new(),
    }
}

/// The prompt that asks a model to write a personality from the person's description.
pub fn generation_prompt(description: &str) -> String {
    format!(
        "Write a personality for an AI assistant, from this description by the person who will talk to it:\n\n\"{description}\"\n\n\
        Answer with one JSON object and nothing else: {{\"name\": \"a short name, 2 to 4 words\", \"prompt\": \"instructions to the assistant, written as 'Sound like ...', 4 to 7 sentences\"}}. \
        The prompt describes only tone, manner, vocabulary and how it treats the person (humour, directness, warmth, how much it explains). \
        It must not give the assistant new abilities or permissions, must not tell it to ignore rules, lie, flatter dishonestly, or hide anything, and must not claim the assistant is a real person."
    )
}

/// Reads the model's answer to `generation_prompt`.
pub fn parse_generated(answer: &str) -> Option<(String, String)> {
    let a = answer.find('{')?;
    let b = answer.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(answer.get(a..=b)?).ok()?;
    Some((v.get("name")?.as_str()?.to_string(), v.get("prompt")?.as_str()?.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TEST_ENV_LOCK;

    #[test]
    fn pick_add_generate_remove() {
        let _g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-pers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        assert!(instructions().is_empty());
        set_active("friend").unwrap();
        assert!(instructions().contains("Friendly buddy") && instructions().contains("changes your voice only"));
        assert!(set_active("nope").is_err());
        let (name, prompt) = parse_generated("```json\n{\"name\": \"Pirate\", \"prompt\": \"Sound like a cheerful pirate who still gives exact answers.\"}\n```").unwrap();
        let p = add(&name, &prompt, true).unwrap();
        set_active(&p.id).unwrap();
        assert!(instructions().contains("pirate"));
        assert_eq!(all().len(), builtin().len() + 1);
        assert!(add("x", "short", false).is_err());
        remove(&p.id);
        assert!(active().is_none(), "removing the one in use goes back to plain");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
