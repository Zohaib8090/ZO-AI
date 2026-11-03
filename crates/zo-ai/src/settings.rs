//! The Settings page: the model and its key, the permission mode, how other apps connect to ZO-AI, and the saved data.

use crate::{chat, chats, hbox, lbl, secondary, set_nav, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::rc::Rc;
use zo_core::ai::{agent, claude_code, providers, Selection};

fn section(title: &str) -> (gtk::Box, gtk::Box) {
    let outer = vbox(8);
    outer.set_margin_bottom(26);
    outer.append(&lbl(title, "settings-head"));
    let card = vbox(0);
    card.add_css_class("settings-card");
    outer.append(&card);
    (outer, card)
}

/// One line in a section: a title and a few words under it on the left, a control on the right.
fn row(title: &str, sub: &str, control: Option<&impl IsA<gtk::Widget>>) -> gtk::Box {
    let r = hbox(14);
    r.add_css_class("settings-row");
    let col = vbox(3);
    col.set_hexpand(true);
    col.set_valign(gtk::Align::Center);
    col.append(&lbl(title, "pc-name"));
    if !sub.is_empty() {
        let s = lbl(sub, "pc-meta");
        s.set_wrap(true);
        s.set_max_width_chars(60);
        col.append(&s);
    }
    r.append(&col);
    if let Some(c) = control {
        c.set_valign(gtk::Align::Center);
        r.append(c);
    }
    r
}

fn no_control() -> Option<&'static gtk::Button> {
    None
}

fn zo_mcp_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("zo-mcp")))
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "zo-mcp".into())
}

/// Builds the page again from what is true now, and shows it.
pub fn show(ui: &Rc<Ui>) {
    let content = &ui.settings_box;
    while let Some(c) = content.first_child() {
        content.remove(&c);
    }
    content.append(&lbl("Settings", "page-title"));
    content.set_spacing(0);
    if let Some(t) = content.first_child() {
        t.set_margin_bottom(24);
    }

    // ---- the model ----
    let sel = Selection::load();
    let (sec, card) = section("Model");
    let provider = providers::find(&sel.provider);
    let current = match provider {
        Some(p) if !sel.model.is_empty() => format!("{} · {}", p.name, sel.model),
        Some(p) => format!("{} · no model chosen", p.name),
        None => "Nothing chosen yet".to_string(),
    };
    let change = secondary("Change");
    let ui2 = ui.clone();
    change.connect_clicked(move |_| chat::models_sheet(&ui2));
    card.append(&row("Provider and model", &current, Some(&change)));
    let claude_state = if claude_code::binary().is_some() {
        "Installed. Choose \"Claude Code\" as the provider to use your own Claude sign-in (claude auth login). ZO-AI never sees it."
    } else {
        "Not installed. Install it with: npm install -g @anthropic-ai/claude-code, then sign in with: claude auth login"
    };
    card.append(&row("Claude Code", claude_state, no_control()));
    let keys_note = if zo_core::ai::keys::available() {
        "API keys are kept in the system keyring, never in a plain file."
    } else {
        "The keyring tool (secret-tool) is missing, so API keys cannot be saved."
    };
    card.append(&row("API keys", keys_note, no_control()));
    content.append(&sec);

    // ---- permissions ----
    let (sec, card) = section("Permissions");
    let modes = ["Manual", "Auto", "Bypass"];
    let dd = gtk::DropDown::from_strings(&modes);
    dd.set_selected(modes.iter().position(|m| *m == zo_core::Mode::load().name()).unwrap_or(1) as u32);
    let ui2 = ui.clone();
    dd.connect_selected_notify(move |d| {
        let name = modes[d.selected() as usize];
        zo_core::Mode::parse(name).save();
        ui2.perm_label.set_text(name);
        ui2.pv_perm_label.set_text(name);
    });
    card.append(&row(
        "Permission mode",
        "Manual asks before every change and command. Auto lets looking around run and asks before changes. Bypass never asks. The same rule applies to every app connected to ZO-AI.",
        Some(&dd),
    ));
    content.append(&sec);

    // ---- other apps ----
    let (sec, card) = section("Connect other apps");
    let cmd = format!("claude mcp add zohara -- {}", zo_mcp_path());
    let copy = secondary("Copy");
    let c2 = cmd.clone();
    copy.connect_clicked(move |b| b.clipboard().set_text(&c2));
    card.append(&row("Let Claude Code control this computer", "Any app that speaks MCP can use ZO-AI's tools (files, shell, screenshots, schedules, projects). Run this once:", Some(&copy)));
    let code = lbl(&cmd, "settings-code");
    code.set_selectable(true);
    code.set_wrap(true);
    code.set_margin_start(16);
    code.set_margin_end(16);
    code.set_margin_bottom(14);
    card.append(&code);
    content.append(&sec);

    // ---- data ----
    let (sec, card) = section("Data");
    let n = chats::load().len();
    let open = secondary("Open folder");
    open.connect_clicked(|b| {
        let dir = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{}/.local/share", std::env::var("HOME").unwrap_or_default()));
        let launcher = gtk::UriLauncher::new(&format!("file://{dir}/zo-ai"));
        let win = b.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        launcher.launch(win.as_ref(), gtk::gio::Cancellable::NONE, |_| {});
    });
    card.append(&row("Saved chats, projects and tasks", &format!("{n} saved chats. Everything ZO-AI does is also written to an action log in the same folder."), Some(&open)));
    let clear = secondary("Delete all chats");
    clear.add_css_class("danger");
    let ui2 = ui.clone();
    clear.connect_clicked(move |b| {
        let dialog = adw::AlertDialog::new(Some("Delete all saved chats?"), Some("This cannot be undone. Projects and scheduled tasks stay."));
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let ui3 = ui2.clone();
        dialog.connect_response(None, move |_, id| {
            if id == "delete" {
                for c in chats::load() {
                    chats::delete(&c.id);
                }
                crate::refresh_recents(&ui3);
                show(&ui3);
            }
        });
        dialog.present(b.root().as_ref());
    });
    card.append(&row("Delete all chats", "Removes the saved conversations from this computer.", Some(&clear)));
    content.append(&sec);

    // ---- about ----
    let (sec, card) = section("About");
    card.append(&row("ZO-AI", &format!("Version {} · the AI built into Zohara OS", env!("CARGO_PKG_VERSION")), no_control()));
    let _ = agent::SYSTEM_PROMPT;
    content.append(&sec);

    ui.stack.set_visible_child_name("settings");
    set_nav(ui, "settings");
    ui.top_title.set_text("Settings");
}
