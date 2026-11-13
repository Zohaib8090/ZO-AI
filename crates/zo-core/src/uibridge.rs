//! A way for tools that run on the agent's thread to ask the Zohara Agent window (its own thread) to do something, such as
//! driving the built-in browser, and wait for the answer.

use serde_json::Value;
use std::sync::OnceLock;

type Handler = Box<dyn Fn(&str, &Value) -> Result<String, String> + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// The window registers the function that does the work (it hands the request over to its own thread and waits).
pub fn register(h: impl Fn(&str, &Value) -> Result<String, String> + Send + Sync + 'static) {
    let _ = HANDLER.set(Box::new(h));
}

pub fn call(name: &str, args: &Value) -> Result<String, String> {
    if crate::tenant::is_restricted() {
        return Err("This needs the Zohara Agent window.".into());
    }
    match HANDLER.get() {
        Some(h) => h(name, args),
        None => Err("This needs the Zohara Agent window.".into()),
    }
}

/// Is a window there to do the work (the browser tools are only offered then)?
pub fn available() -> bool {
    if crate::tenant::is_restricted() {
        return false;
    }
    HANDLER.get().is_some()
}
