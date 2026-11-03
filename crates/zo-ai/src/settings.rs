//! The Settings page: the model and its key, the permission mode, how other apps connect to ZO-AI, and the saved data.

use crate::{chat, chats, hbox, lbl, secondary, set_nav, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::rc::Rc;
use zo_core::ai::{agent, claude_code, providers, Selection};
use zo_core::mcp_client::{self, ServerConfig};
use std::sync::mpsc;

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

    // ---- looks ----
    let (sec, card) = section("Appearance");
    let looks = ["system", "dark", "light"];
    let dd = gtk::DropDown::from_strings(&["Follow the system", "Dark", "Light"]);
    dd.set_selected(looks.iter().position(|l| *l == crate::theme::choice()).unwrap_or(0) as u32);
    dd.connect_selected_notify(move |d| crate::theme::set(looks[d.selected() as usize]));
    card.append(&row("Theme", "Light or dark, or the same as the rest of the system.", Some(&dd)));
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

    // ---- MCP servers ZO-AI can use ----
    let (sec, card) = section("MCP servers ZO-AI can use");
    let servers = mcp_client::load_servers();
    let add = secondary("Add server");
    let ui2 = ui.clone();
    add.connect_clicked(move |_| add_server_sheet(&ui2));
    card.append(&row("Add tools from other programs", "Add an MCP server (for example a GitHub or a database server) and the chat can use its tools. Every call asks first, unless the mode is Bypass.", Some(&add)));
    for s in servers {
        let r = hbox(12);
        r.add_css_class("settings-row");
        let col = vbox(3);
        col.set_hexpand(true);
        col.append(&lbl(&s.name, "pc-name"));
        let cmdl = lbl(&s.command, "pc-meta");
        cmdl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        col.append(&cmdl);
        r.append(&col);
        let sw = gtk::Switch::new();
        sw.set_active(s.enabled);
        sw.set_valign(gtk::Align::Center);
        let name = s.name.clone();
        sw.connect_state_set(move |_, on| {
            let mut all = mcp_client::load_servers();
            if let Some(x) = all.iter_mut().find(|x| x.name == name) {
                x.enabled = on;
            }
            mcp_client::save_servers(&all);
            gtk::glib::Propagation::Proceed
        });
        r.append(&sw);
        let del = crate::icon_button("edit-delete-symbolic", "Remove this server", "icon-btn");
        let (ui2, name) = (ui.clone(), s.name.clone());
        del.connect_clicked(move |_| {
            let mut all = mcp_client::load_servers();
            all.retain(|x| x.name != name);
            mcp_client::save_servers(&all);
            show(&ui2);
        });
        r.append(&del);
        card.append(&r);
    }
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

/// "Add server": a name and a command. It is started once to see that it works before it is saved.
fn add_server_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = crate::sheet(ui, "Add an MCP server", 560);
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("github"));
    name.add_css_class("field-input");
    body.append(&crate::field("Name", &name));
    let command = gtk::Entry::new();
    command.set_placeholder_text(Some("npx -y @modelcontextprotocol/server-github"));
    command.add_css_class("field-input");
    body.append(&crate::field("Command that starts it", &command));
    let status = lbl("The server is started once to check that it works. Servers that need a key read it from the environment you start ZO-AI in.", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);
    let cancel = secondary("Cancel");
    let add = crate::primary("Test and add");
    foot.append(&cancel);
    foot.append(&add);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    {
        let (w, ui2) = (win.clone(), ui.clone());
        add.connect_clicked(move |b| {
            let cfg = ServerConfig { name: mcp_client::clean_name(&name.text()), command: command.text().trim().to_string(), enabled: true };
            if cfg.name.is_empty() || cfg.command.is_empty() {
                status.set_text("Give it a name and a command.");
                return;
            }
            if mcp_client::load_servers().iter().any(|s| s.name == cfg.name) {
                status.set_text("A server with that name is already added.");
                return;
            }
            status.set_text("Starting it…");
            b.set_sensitive(false);
            let (tx, rx) = mpsc::channel();
            let cfg2 = cfg.clone();
            std::thread::spawn(move || {
                let _ = tx.send(mcp_client::test(&cfg2));
            });
            let (status, b, w, ui3) = (status.clone(), b.clone(), w.clone(), ui2.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(150), move || match rx.try_recv() {
                Ok(Ok(tools)) => {
                    let mut all = mcp_client::load_servers();
                    all.push(cfg.clone());
                    mcp_client::save_servers(&all);
                    w.close();
                    show(&ui3);
                    let _ = tools;
                    gtk::glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    status.set_text(&format!("It did not work: {e}"));
                    b.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(_) => {
                    b.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                }
            });
        });
    }
    win.present();
}
