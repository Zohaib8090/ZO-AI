//! Friendly answers about a provider for the screens that show them (desktop and web): the model list with a readable reason when it
//! cannot be loaded, and a connection test. Nothing here changes any setting.

use super::agent::{self, Config};
use super::providers::{Protocol, Provider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How long the connection test waits for the provider.
pub const TEST_WAIT: Duration = Duration::from_secs(15);

/// The address must be an http(s) address without spaces; this is only a shape check (a restricted account has its own, stricter rule).
pub fn check_address(base_url: &str) -> Result<(), String> {
    let u = base_url.trim();
    let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"));
    match rest {
        Some(r) if !r.is_empty() && r.len() <= 500 && !u.chars().any(|c| c.is_whitespace() || c.is_control()) && !r.starts_with('/') => Ok(()),
        _ => Err("The address must start with http:// or https:// and have no spaces (for example https://api.example.com/v1).".into()),
    }
}

/// A provider's error turned into a sentence a person can act on: no key, bad address, 401, timeout.
pub fn friendly(p: &Provider, base_url: &str, has_key: bool, err: &str) -> String {
    let low = err.to_lowercase();
    let name = p.name;
    if err.starts_with("A restricted account") || err.starts_with("Claude Code is not available") {
        return err.to_string();
    }
    if low.contains("answered 401") || low.contains("answered 403") {
        return if has_key || p.local {
            format!("{name} refused the key (401). Check that the key is right and belongs to {name}.")
        } else {
            format!("No API key for {name} yet: add it in the model settings, then load the models again.")
        };
    }
    if low.contains("answered 404") {
        return format!("Nothing answers at {}/models: check the address (it usually ends in /v1).", base_url.trim_end_matches('/'));
    }
    if low.contains("answered 429") {
        return format!("{name} says there are too many requests right now. Wait a moment and try again.");
    }
    if low.contains("answered 5") {
        return format!("{name} had a problem on its side ({}). Try again later.", err.split("answered ").nth(1).and_then(|r| r.get(..3)).unwrap_or("5xx"));
    }
    if low.contains("timed out") || low.contains("timeout") || low.contains("deadline") {
        return format!("{name} did not answer in time. Check the address and your connection.");
    }
    if low.contains("dns") || low.contains("lookup") || low.contains("resolve") {
        return "That address could not be found: check the host name.".into();
    }
    if low.contains("refused") {
        return "Nothing is listening at that address (connection refused): is the server running, and is the port right?".into();
    }
    if low.contains("could not reach") || low.contains("connection") || low.contains("tls") || low.contains("certificate") {
        return format!("Cannot reach {}: check the address and your connection.", base_url.trim_end_matches('/'));
    }
    if low.contains("expected") || low.contains("eof") || low.contains("invalid") && low.contains("json") {
        return format!("{} answered, but not like a model server (an OpenAI-compatible one lists its models at /models).", base_url.trim_end_matches('/'));
    }
    err.to_string()
}

/// The models a provider offers. A failure is a readable `Err`, never a panic or a raw error.
pub fn list(p: &Provider, base_url: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    if p.protocol != Protocol::ClaudeCode {
        check_address(base_url)?;
    }
    agent::list_models(p, base_url, key).map_err(|e| friendly(p, base_url, key.is_some(), &e))
}

/// A real, tiny test: with a model, one short answer is asked for (15 seconds at most); without one, the model list is loaded.
/// Ok is what to tell the person; Err is the friendly reason it failed.
pub fn test(p: &'static Provider, base_url: &str, key: Option<String>, model: &str) -> Result<String, String> {
    if p.protocol == Protocol::ClaudeCode {
        return Ok("Claude Code runs on this computer with your own sign-in; there is no address to test here.".into());
    }
    check_address(base_url)?;
    let has_key = key.is_some();
    if model.trim().is_empty() {
        let n = list(p, base_url, key.as_deref())?.len();
        return Ok(format!("Connected to {}: {n} model{} available.", p.name, if n == 1 { "" } else { "s" }));
    }
    let cfg = Config { provider: p, base_url: base_url.to_string(), model: model.trim().to_string(), key, system: "Reply with the single word: ok".into(), vision: false };
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let c2 = cancel.clone();
    crate::tenant::spawn(move || {
        let _ = tx.send(agent::complete(&cfg, &cfg.system, "ok?", &c2));
    });
    match rx.recv_timeout(TEST_WAIT) {
        Ok(Ok(_)) => Ok(format!("{} answered with {}: it works.", p.name, model.trim())),
        Ok(Err(e)) => Err(friendly(p, base_url, has_key, &e)),
        Err(_) => {
            cancel.store(true, Ordering::SeqCst);
            Err(format!("{} did not answer within {} seconds.", p.name, TEST_WAIT.as_secs()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::providers;

    #[test]
    fn errors_become_sentences_a_person_can_act_on() {
        let custom = providers::find("custom").unwrap();
        let o = providers::find("openai").unwrap();
        assert!(friendly(o, "https://x/v1", false, "The provider answered 401: bad").contains("No API key"));
        assert!(friendly(o, "https://x/v1", true, "The provider answered 401: bad").contains("refused the key"));
        assert!(friendly(custom, "https://x/v1/", true, "The provider answered 404: nope").contains("https://x/v1/models"));
        assert!(friendly(o, "https://x", true, "Could not reach the provider: https://x/models: Dns Failed: no such host").contains("host name"));
        assert!(friendly(o, "https://x", true, "Could not reach the provider: Connection refused").contains("connection refused"));
        assert!(friendly(o, "https://x", true, "Could not reach the provider: timed out reading response").contains("in time"));
        assert!(friendly(o, "https://x", true, "The provider answered 503: x").contains("503"));
        assert!(check_address("ftp://x").is_err() && check_address("https://").is_err() && check_address("https://a b").is_err());
        assert!(check_address("http://127.0.0.1:8000/v1").is_ok() && check_address(" https://api.example.com/v1 ").is_ok());
    }
}
