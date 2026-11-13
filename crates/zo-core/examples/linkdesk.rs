//! A stand-in for the desktop app's side of the desktop link, for the end-to-end test in zo-server (never installed).
//! It starts the real Zohara Chat service on a socket in $XDG_RUNTIME_DIR and runs the real link loop (`zo_core::link::run_loop`) against a server.
//! A sandbox is needed: the caller sets HOME, XDG_DATA_HOME, XDG_CONFIG_HOME and XDG_RUNTIME_DIR.
//!
//!   LINK_BASE     the server (http://127.0.0.1:port)
//!   LINK_CRED     the desktop's device credential
//!   LINK_MODEL    the base URL of an OpenAI-compatible model server (the mock-model example)
//!   LINK_ANSWERS  what "the person on this computer" answers to approval dialogs, in order: "y,n,y" (default: no)
//!   LINK_LOG      a file that gets one line per approval dialog ("ASK <summary>")

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

struct Dialog {
    answers: Mutex<Vec<bool>>,
    log: String,
}

impl zo_core::link::Host for Dialog {
    fn ask_local(&self, summary: &str) -> bool {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.log) {
            let _ = writeln!(f, "ASK {}", summary.replace('\n', " "));
        }
        let mut a = self.answers.lock().unwrap();
        if a.is_empty() {
            false
        } else {
            a.remove(0)
        }
    }
}

fn main() {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (base, cred) = (env("LINK_BASE"), env("LINK_CRED"));
    let mut sel = zo_core::ai::Selection { provider: "ollama".into(), model: "m".into(), ..Default::default() };
    sel.base_urls.insert("ollama".into(), env("LINK_MODEL"));
    sel.save();
    zo_core::policy::Mode::Manual.save();
    zo_core::service::start().expect("service");
    let answers = env("LINK_ANSWERS").split(',').filter(|s| !s.is_empty()).map(|s| s == "y").collect();
    let host = Arc::new(Dialog { answers: Mutex::new(answers), log: env("LINK_LOG") });
    let ep: zo_core::link::Endpoint = Arc::new(move || Some((base.clone(), cred.clone())));
    println!("READY");
    std::io::stdout().flush().unwrap();
    zo_core::link::run_loop(host, ep, zo_core::link::default_connect(), Arc::new(AtomicBool::new(false)));
}
