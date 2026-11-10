//! Writes a personality from a description, with the model that is chosen for chatting (no tools).

use super::agent::{self, Config};
use super::{claude_code, Selection};
use crate::store::personality;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const SYSTEM: &str = "You write personalities for an AI assistant. You answer with one JSON object and nothing else.";

/// Asks the model and saves the result as a new personality (not switched on). Blocks until the model answers: run it off the main thread.
pub fn generate(description: &str) -> Result<personality::Personality, String> {
    let description = description.trim();
    if description.chars().count() < 3 {
        return Err("Describe the personality in a few words.".into());
    }
    let sel = Selection::load();
    let prompt = personality::generation_prompt(description);
    let answer = if sel.provider == "claude-code" {
        claude_code::complete(if sel.model.is_empty() { "sonnet" } else { &sel.model }, SYSTEM, &prompt, &Arc::new(AtomicBool::new(false)))?
    } else {
        let cfg = Config::from_selection(&sel, SYSTEM)?;
        agent::complete(&cfg, SYSTEM, &prompt, &AtomicBool::new(false))?
    };
    let (name, text) = personality::parse_generated(&answer).ok_or("The model's answer was not a personality. Try again.")?;
    personality::add(&name, &text, true)
}
