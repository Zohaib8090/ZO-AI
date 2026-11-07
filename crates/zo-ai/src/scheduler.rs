//! Runs scheduled tasks when their time comes, on their own, and leaves each result as a chat under Recents.

use crate::chats::{self, Chat, Entry};
use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use zo_core::ai::agent::{self, Config, Event};
use zo_core::ai::claude_code;
use zo_core::ai::wire::Turn;
use zo_core::ai::Selection;
use zo_core::store::schedules::{self, Task};

fn entry(kind: &str, text: impl Into<String>) -> Entry {
    Entry { kind: kind.into(), text: text.into() }
}

/// Does one task now and returns the chat it leaves behind.
fn run_task(task: &Task) -> Chat {
    let sel = Selection::load();
    let now = schedules::now_millis();
    let mut entries = vec![entry("msg-user", task.prompt.clone())];
    let system = format!(
        "{}\n\nThis is a scheduled task. It is running on its own at the planned time and nobody is watching. Do it, then say briefly what you did.",
        agent::SYSTEM_PROMPT
    );
    let mut history: Vec<Turn> = Vec::new();
    let mut session = None;
    let result = {
        let mut on = |e: Event| match e {
            Event::TextDelta(_) | Event::Usage(_) => {}
            Event::Text(t) => entries.push(entry("msg-ai", t)),
            Event::ToolStart { name, summary } => entries.push(entry("msg-tool", format!("▸ {name}  {}", summary.lines().nth(1).unwrap_or("")))),
            Event::ToolDone { ok, output, .. } => {
                let short: String = output.chars().take(300).collect();
                entries.push(entry(if ok { "msg-tool-out" } else { "msg-error" }, format!("{} {short}", if ok { "✓" } else { "✗" })));
            }
        };
        if sel.provider == "claude-code" {
            let model = if sel.model.is_empty() { "sonnet".to_string() } else { sel.model.clone() };
            claude_code::run(&model, sel.effort.as_deref(), &mut session, &task.prompt, &system, &Arc::new(AtomicBool::new(false)), &mut on)
        } else {
            match Config::from_selection(&sel, &system) {
                Ok(cfg) => agent::run(&cfg, &mut history, &task.prompt, zo_core::Mode::load(), &zo_core::approval::AppApprover, &AtomicBool::new(false), &mut on),
                Err(e) => Err(e),
            }
        }
    };
    if let Err(e) = result {
        entries.push(entry("msg-error", e));
    }
    Chat { id: format!("c{now}"), title: format!("Scheduled: {}", task.title), updated: now, entries, history, session, context: String::new(), pinned: false, branches: Default::default() }
}

thread_local! {
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Looks for tasks that are due and runs them one after another in the background. `done` is called when they have finished.
pub fn check(done: Rc<dyn Fn()>) {
    if RUNNING.with(Cell::get) {
        return;
    }
    let due = schedules::due(schedules::now_millis());
    if due.is_empty() {
        return;
    }
    RUNNING.with(|r| r.set(true));
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for task in due {
            let chat = run_task(&task);
            let id = chat.id.clone();
            chats::upsert(chat);
            schedules::mark_ran(&task.id, schedules::now_millis(), Some(id));
            let _ = tx.send(task.title);
        }
    });
    glib::timeout_add_local(Duration::from_millis(500), move || match rx.try_recv() {
        Ok(title) => {
            if let Some(app) = gtk::gio::Application::default() {
                let n = gtk::gio::Notification::new("ZO-AI finished a scheduled task");
                n.set_body(Some(&title));
                app.send_notification(None, &n);
            }
            done();
            glib::ControlFlow::Continue
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            RUNNING.with(|r| r.set(false));
            done();
            glib::ControlFlow::Break
        }
    });
}

/// Starts looking every 20 seconds.
pub fn start(done: Rc<dyn Fn()>) {
    let d = done.clone();
    glib::timeout_add_seconds_local(20, move || {
        check(d.clone());
        glib::ControlFlow::Continue
    });
    check(done);
}
