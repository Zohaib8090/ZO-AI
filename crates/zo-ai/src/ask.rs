//! The approval dialog: when a tool needs a yes (from the MCP server or from the app itself), this asks.

use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use libadwaita as adw;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// How many approval questions are open right now (the tray asks for attention while it is above 0).
static PENDING: AtomicUsize = AtomicUsize::new(0);

pub fn pending() -> usize {
    PENDING.load(Ordering::SeqCst)
}

/// Start listening for approval questions. The answer comes from a dialog on the main thread.
pub fn start() -> bool {
    let result = zo_core::approval::serve(|tool, summary| {
        let (tx, rx) = mpsc::channel::<bool>();
        glib::MainContext::default().invoke(move || show(&tool, &summary, tx));
        rx.recv_timeout(Duration::from_secs(120)).unwrap_or(false)
    });
    if let Err(e) = &result {
        eprintln!("zo-ai: cannot listen for approvals: {e}");
    }
    result.is_ok()
}

fn show(tool: &str, summary: &str, tx: mpsc::Sender<bool>) {
    let dialog = adw::AlertDialog::new(Some("Allow ZO-AI to do this?"), Some(summary));
    dialog.add_responses(&[("deny", "Don't allow"), ("allow", "Allow once")]);
    dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("deny"));
    dialog.set_close_response("deny");
    dialog.set_body_use_markup(false);
    let _ = tool;
    PENDING.fetch_add(1, Ordering::SeqCst);
    dialog.connect_response(None, move |_, id| {
        PENDING.fetch_sub(1, Ordering::SeqCst);
        let _ = tx.send(id == "allow");
    });
    let parent = gtk::gio::Application::default()
        .and_then(|a| a.downcast::<gtk::Application>().ok())
        .and_then(|a| a.active_window());
    dialog.present(parent.as_ref());
}
