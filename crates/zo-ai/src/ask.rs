//! The approval dialog: when a tool needs a yes (from the MCP server or from the app itself), this asks.

use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// How many approval questions are open right now (the tray asks for attention while it is above 0).
static PENDING: AtomicUsize = AtomicUsize::new(0);
/// How many question cards (`zohara_ask_user`) wait for an answer: they count for the tray too.
static QUESTIONS: AtomicUsize = AtomicUsize::new(0);

/// Approvals and question cards that wait for the person.
pub fn pending() -> usize {
    PENDING.load(Ordering::SeqCst) + QUESTIONS.load(Ordering::SeqCst)
}

/// A question card was shown (`true`) or answered, skipped or ended (`false`).
pub fn question_open(open: bool) {
    if open {
        QUESTIONS.fetch_add(1, Ordering::SeqCst);
    } else {
        if QUESTIONS.load(Ordering::SeqCst) > 0 {
            QUESTIONS.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

thread_local! {
    /// Approval dialogs that are open (main thread only): so that a button of the notification can answer one, and the next id.
    static OPEN: RefCell<HashMap<u32, (mpsc::Sender<bool>, adw::AlertDialog)>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u32> = const { Cell::new(1) };
}

/// The window is not in front: closed into the tray, minimized, or another program has the focus.
fn away() -> bool {
    let win = gtk::gio::Application::default().and_then(|a| a.downcast::<gtk::Application>().ok()).and_then(|a| a.active_window());
    !win.is_some_and(|w| w.is_active() && w.is_visible())
}

/// "ZO-AI needs you": a desktop notification (org.freedesktop.Notifications, through GIO), only when the window is not in front.
/// An approval has Allow and Deny buttons (`approval`: the id of the dialog); clicking the body brings the window forward, where the
/// card or the dialog waits. Main thread only.
pub fn needs_you(tag: &str, body: &str, approval: Option<u32>) {
    if !away() {
        return;
    }
    let Some(app) = gtk::gio::Application::default() else { return };
    let n = gtk::gio::Notification::new("ZO-AI needs you");
    n.set_body(Some(body));
    n.set_priority(gtk::gio::NotificationPriority::High);
    n.set_default_action("app.zo-show");
    if let Some(id) = approval {
        n.add_button_with_target_value("Allow", "app.zo-approve", Some(&(id, true).to_variant()));
        n.add_button_with_target_value("Deny", "app.zo-approve", Some(&(id, false).to_variant()));
    } else {
        n.add_button("Answer", "app.zo-show");
    }
    app.send_notification(Some(tag), &n);
}

/// Takes a notification back (the person answered in the window).
pub fn withdraw(tag: &str) {
    if let Some(app) = gtk::gio::Application::default() {
        app.withdraw_notification(tag);
    }
}

/// The actions the notification buttons call: `app.zo-show` (bring the window forward) and `app.zo-approve` ((dialog id, yes)).
pub fn init(app: &adw::Application) {
    let show = gtk::gio::SimpleAction::new("zo-show", None);
    show.connect_activate(|_, _| {
        if let Some(app) = gtk::gio::Application::default() {
            app.activate();
        }
    });
    app.add_action(&show);
    let approve = gtk::gio::SimpleAction::new("zo-approve", Some(glib::VariantTy::new("(ub)").unwrap()));
    approve.connect_activate(|_, param| {
        let Some((id, yes)) = param.and_then(|p| p.get::<(u32, bool)>()) else { return };
        if let Some((tx, dialog)) = OPEN.with(|o| o.borrow_mut().remove(&id)) {
            PENDING.fetch_sub(1, Ordering::SeqCst);
            let _ = tx.send(yes);
            dialog.force_close();
        }
        withdraw(&format!("zo-approval-{id}"));
    });
    app.add_action(&approve);
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

/// The desktop link asks here when a web session wants to do something on this computer. Called from the link's thread: the
/// dialog is shown by the main context, and an unanswered question counts as no after three minutes. Counts as an open approval,
/// so the tray asks for attention.
pub struct LinkHost;

impl zo_core::link::Host for LinkHost {
    fn ask_local(&self, summary: &str) -> bool {
        let (tx, rx) = mpsc::channel::<bool>();
        let text = format!("A web session wants to: {summary}");
        glib::MainContext::default().invoke(move || show_with("Allow this web session?", &text, "Deny", "Allow", tx));
        rx.recv_timeout(Duration::from_secs(180)).unwrap_or(false)
    }
}

fn show(tool: &str, summary: &str, tx: mpsc::Sender<bool>) {
    let _ = tool;
    show_with("Allow ZO-AI to do this?", summary, "Don't allow", "Allow once", tx);
}

fn show_with(title: &str, summary: &str, deny: &str, allow: &str, tx: mpsc::Sender<bool>) {
    let dialog = adw::AlertDialog::new(Some(title), Some(summary));
    dialog.add_responses(&[("deny", deny), ("allow", allow)]);
    dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("deny"));
    dialog.set_close_response("deny");
    dialog.set_body_use_markup(false);
    PENDING.fetch_add(1, Ordering::SeqCst);
    let did = NEXT.with(|n| n.replace(n.get().wrapping_add(1)));
    OPEN.with(|o| o.borrow_mut().insert(did, (tx, dialog.clone())));
    dialog.connect_response(None, move |_, id| {
        // the notification may have answered already: then the entry is gone and nothing is sent twice
        if let Some((tx, _)) = OPEN.with(|o| o.borrow_mut().remove(&did)) {
            PENDING.fetch_sub(1, Ordering::SeqCst);
            let _ = tx.send(id == "allow");
        }
        withdraw(&format!("zo-approval-{did}"));
    });
    let parent = gtk::gio::Application::default()
        .and_then(|a| a.downcast::<gtk::Application>().ok())
        .and_then(|a| a.active_window());
    dialog.present(parent.as_ref());
    needs_you(&format!("zo-approval-{did}"), summary, Some(did));
}
