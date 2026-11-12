//! How full the model's window is (estimated), and when a conversation should be compacted.
//!
//! There is no tokenizer here: a token is taken to be about four characters, which is close enough to show where the space goes and to
//! decide when to compact. The provider's own count of the last request (`wire::Usage`) is shown next to it.

use super::providers::Provider;
use super::wire::{ToolSpec, Turn};

/// A conversation is compacted when it reaches this share of the window.
pub const COMPACT_AT: f64 = 0.85;
/// What a picture costs, about.
pub const IMAGE_TOKENS: u64 = 1500;

pub fn tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

pub fn turn_tokens(t: &Turn) -> u64 {
    match t {
        Turn::User(s) => tokens(s) + 4,
        Turn::UserImages(s, imgs) => tokens(s) + 4 + imgs.len() as u64 * IMAGE_TOKENS,
        Turn::UserInline(s, imgs) => tokens(s) + 4 + imgs.len() as u64 * IMAGE_TOKENS,
        Turn::Assistant { text, calls } => tokens(text) + calls.iter().map(|c| tokens(&c.name) + tokens(&c.args.to_string()) + 8).sum::<u64>() + 4,
        Turn::Results(rs) => rs.iter().map(|r| tokens(&r.content) + tokens(&r.name) + 8 + r.images.len() as u64 * IMAGE_TOKENS).sum(),
    }
}

pub fn history_tokens(h: &[Turn]) -> u64 {
    h.iter().map(turn_tokens).sum()
}

/// What a list of tools costs: each one's name, description and parameters are sent with every request.
pub fn specs_tokens(specs: &[ToolSpec]) -> u64 {
    specs.iter().map(|s| tokens(&s.name) + tokens(&s.description) + tokens(&s.schema.to_string()) + 8).sum()
}

/// The size of the model's window: what the Models API said, else a sensible size for the kind of model.
pub fn window_for(p: &Provider, model: &str) -> u64 {
    if let Some(n) = super::models::claude_models().into_iter().find(|m| m.id == model).and_then(|m| m.max_input_tokens) {
        return n;
    }
    let m = model.to_lowercase();
    match p.id {
        "anthropic" | "claude-code" => if m.contains("[1m]") || m.contains("-5") || m.contains("opus-4-6") || m.contains("opus-4-7") || m.contains("opus-4-8") { 1_000_000 } else { 200_000 },
        "gemini" => 1_000_000,
        "openai" => if m.contains("gpt-5") || m.contains("gpt-4.1") { 400_000 } else { 128_000 },
        _ if p.local => 32_000,
        _ => 128_000,
    }
}

/// Where the window goes, for the Context panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakdown {
    pub window: u64,
    /// (name, tokens) of what is in the window now.
    pub items: Vec<(&'static str, u64)>,
    pub used: u64,
    /// What is kept free for the summary and the reply: from the compaction point to the end of the window.
    pub buffer: u64,
    pub free: u64,
    /// How much more fits before the conversation is compacted by itself.
    pub until_compact: u64,
}

pub fn breakdown(window: u64, messages: u64, system_tools: u64, mcp_tools: u64, system_prompt: u64, mcp_instructions: u64, memory: u64) -> Breakdown {
    let items = vec![
        ("Messages", messages),
        ("System tools", system_tools),
        ("MCP tools", mcp_tools),
        ("System prompt", system_prompt),
        ("MCP server instructions", mcp_instructions),
        ("Memory and project", memory),
    ];
    let used: u64 = items.iter().map(|(_, t)| t).sum();
    let limit = (window as f64 * COMPACT_AT) as u64;
    Breakdown { window, used, buffer: window.saturating_sub(limit), free: limit.saturating_sub(used), until_compact: limit.saturating_sub(used), items }
}

/// Readable numbers: 798100 -> "798.1k", 1000000 -> "1M".
pub fn short(n: u64) -> String {
    if n >= 1_000_000 {
        let m = n as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 { format!("{}M", m.round() as u64) } else { format!("{m:.1}M") }
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::wire::{ImageRef, ToolCall, ToolResult};
    use serde_json::json;

    #[test]
    fn tokens_are_about_a_quarter_of_the_characters_and_pictures_count_more() {
        assert_eq!(tokens(""), 0);
        assert_eq!(tokens("abcd"), 1);
        assert_eq!(tokens("abcde"), 2);
        let pic = ImageRef { path: "a.png".into(), mime: "image/png".into() };
        assert!(turn_tokens(&Turn::UserImages("hi".into(), vec![pic.clone()])) > IMAGE_TOKENS);
        let calls = vec![ToolCall { id: "1".into(), name: "zohara_shell".into(), args: json!({ "cmd": "ls" }) }];
        assert!(turn_tokens(&Turn::Assistant { text: String::new(), calls }) > 8);
        assert!(history_tokens(&[Turn::Results(vec![ToolResult { id: "1".into(), name: "n".into(), content: "x".repeat(400), is_error: false, images: vec![pic] }])]) > IMAGE_TOKENS + 100);
    }

    #[test]
    fn the_breakdown_adds_up_and_says_how_much_fits_before_compaction() {
        let b = breakdown(1_000_000, 394_200, 20_400, 19_500, 5_700, 1_900, 756);
        assert_eq!(b.used, 442_456);
        assert_eq!(b.buffer, 150_000, "the last 15% is kept for the summary and the reply");
        assert_eq!(b.until_compact, 850_000 - 442_456);
        assert_eq!(b.items.len(), 6);
        assert_eq!(breakdown(1000, 900, 0, 0, 0, 0, 0).until_compact, 0, "already past the point");
    }

    #[test]
    fn numbers_are_written_short() {
        assert_eq!((short(798_100), short(1_000_000), short(950), short(20_400)), ("798.1k".to_string(), "1M".to_string(), "950".to_string(), "20.4k".to_string()));
    }
}
