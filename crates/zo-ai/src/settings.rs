//! The Settings page: the model and its key, the permission mode, how other apps connect to ZO-AI, and the saved data.

use crate::{chat, chats, hbox, lbl, secondary, set_nav, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::rc::Rc;
use zo_core::ai::{agent, claude_code, profiles, providers, Selection};
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
    let profile_name = sel.profile.as_deref().and_then(|id| profiles::load().into_iter().find(|p| p.id == id)).map(|p| p.name);
    let current = match provider {
        Some(p) if !sel.model.is_empty() => format!("{} · {}{}", p.name, sel.model, profile_name.as_ref().map(|n| format!(" · profile {n}")).unwrap_or_default()),
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

    // ---- profiles: presets of provider + model + key ----
    let (sec, card) = section("Profiles");
    {
        let all = profiles::load();
        if all.is_empty() {
            card.append(&row("No profiles yet", "A profile is a preset: a provider, a model, its address, how hard it thinks and its own API key. Make several (one for OpenRouter, one for Gemini, two accounts of the same provider) and switch between them here. Use Change above to set the provider, model and key; with a profile in use they are saved into it.", no_control()));
        }
        for p in all {
            let active = sel.profile.as_deref() == Some(p.id.as_str());
            let controls = hbox(6);
            let use_btn = if active {
                let b = secondary("In use");
                b.set_sensitive(false);
                b
            } else {
                crate::primary("Use")
            };
            {
                let (ui2, p2) = (ui.clone(), p.clone());
                use_btn.connect_clicked(move |_| {
                    let mut s = Selection::load();
                    s.apply_profile(&p2);
                    s.save();
                    ui2.model_label.set_text(&chat::pill_text(&s));
                    ui2.pv_model_label.set_text(&chat::pill_text(&s));
                    show(&ui2);
                });
            }
            controls.append(&use_btn);
            let rename = crate::icon_button("document-edit-symbolic", "Rename this profile", "icon-btn");
            {
                let (ui2, p2) = (ui.clone(), p.clone());
                rename.connect_clicked(move |_| {
                    let (ui3, id) = (ui2.clone(), p2.id.clone());
                    chat::name_sheet(&ui2, "Rename the profile", &p2.name, "Rename", move |n| {
                        profiles::rename(&id, &n)?;
                        show(&ui3);
                        Ok(())
                    });
                });
            }
            controls.append(&rename);
            let del = crate::icon_button("edit-delete-symbolic", "Delete this profile and its key", "icon-btn");
            {
                let (ui2, p2) = (ui.clone(), p.clone());
                del.connect_clicked(move |_| {
                    let (ui3, id) = (ui2.clone(), p2.id.clone());
                    crate::confirm_delete(&ui2, &format!("Delete the profile \"{}\"?", p2.name), "Its saved choices and the API key it kept are removed from this computer.", move || {
                        profiles::delete(&id);
                        let s = Selection::load();
                        ui3.model_label.set_text(&chat::pill_text(&s));
                        show(&ui3);
                    });
                });
            }
            controls.append(&del);
            card.append(&row(&p.name, &chat::describe(&p), Some(&controls)));
            // the models pinned to the model menu, each removable
            if !p.pinned.is_empty() {
                let chips = gtk::FlowBox::new();
                chips.set_selection_mode(gtk::SelectionMode::None);
                chips.set_margin_start(18);
                chips.set_margin_end(18);
                chips.set_margin_bottom(10);
                chips.set_max_children_per_line(4);
                for model in &p.pinned {
                    let chip = hbox(4);
                    chip.add_css_class("pill");
                    chip.append(&lbl(&chat::nice_name(model), ""));
                    let x = crate::icon_button("window-close-symbolic", "Remove from the model menu", "icon-btn");
                    let (ui2, id, m) = (ui.clone(), p.id.clone(), model.clone());
                    x.connect_clicked(move |_| {
                        profiles::unpin(&id, &m);
                        show(&ui2);
                    });
                    chip.append(&x);
                    chips.insert(&chip, -1);
                }
                card.append(&lbl("Pinned to the model menu", "field-hint"));
                card.append(&chips);
            }
        }
        let new = secondary("New profile");
        {
            let ui2 = ui.clone();
            new.connect_clicked(move |_| {
                let ui3 = ui2.clone();
                chat::name_sheet(&ui2, "New profile", "", "Save", move |n| {
                    let mut s = Selection::load();
                    let p = profiles::create(&n, &s)?;
                    s.profile = Some(p.id);
                    s.save();
                    show(&ui3);
                    Ok(())
                });
            });
        }
        card.append(&row("Save the current settings as a profile", "Keeps the provider, model, address, effort and the API key that is in use now.", Some(&new)));
    }
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
    let import = secondary("Paste a config");
    let ui2 = ui.clone();
    import.connect_clicked(move |_| import_servers_sheet(&ui2));
    let both = hbox(8);
    both.append(&import);
    both.append(&add);
    card.append(&row("Add tools from other programs", "Add an MCP server (a program, or a remote one by its address) and the chat can use its tools. Tools that only read run on their own; the rest ask first, unless the mode is Bypass.", Some(&both)));
    let browser = gtk::Entry::new();
    browser.set_text(&mcp_client::browser_command());
    browser.set_placeholder_text(Some("empty: the system's default browser"));
    browser.set_width_chars(34);
    browser.add_css_class("field-input");
    browser.connect_changed(|e| mcp_client::set_browser_command(&e.text()));
    card.append(&row("Browser for sign-in links", "Where a server's sign-in page opens. Empty uses the system's default browser. To use one Chrome profile: google-chrome-stable --profile-directory=\"Default\" %u (the address goes where %u is). The sign-in window also shows the link, so you can copy it into any browser.", Some(&browser)));
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
            let (ui3, name3) = (ui2.clone(), name.clone());
            crate::confirm_delete(&ui2, &format!("Remove \"{name}\"?"), "ZO-AI stops using this server and forgets its sign-in.", move || {
                let mut all = mcp_client::load_servers();
                all.retain(|x| x.name != name3);
                mcp_client::save_servers(&all);
                zo_core::ai::keys::clear(&mcp_client::token_id(&name3));
                show(&ui3);
            });
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

/// For verification: opens the "Add server" panel straight away.
pub(crate) fn open_add_server(ui: &Rc<Ui>) {
    add_server_sheet(ui);
}

/// "Add server": a name and a command. It is started once to see that it works before it is saved.
fn add_server_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = crate::sheet(ui, "Add an MCP server", 560);
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("github"));
    name.add_css_class("field-input");
    body.append(&crate::field("Name", &name));
    let command = gtk::Entry::new();
    command.set_placeholder_text(Some("https://example.com/mcp   or   npx -y @modelcontextprotocol/server-github"));
    command.add_css_class("field-input");
    body.append(&crate::field("Address of the server, or the command that starts it", &command));
    let token = gtk::PasswordEntry::new();
    token.set_show_peek_icon(true);
    token.add_css_class("field-input");
    body.append(&crate::field("Access token (only if the server gave you one)", &token));
    // advanced: what other apps' configuration files hold too
    let env = gtk::TextView::new();
    env.add_css_class("field-input");
    env.set_size_request(-1, 56);
    let headers = gtk::TextView::new();
    headers.add_css_class("field-input");
    headers.set_size_request(-1, 56);
    let client_id = gtk::Entry::new();
    client_id.add_css_class("field-input");
    let client_secret = gtk::PasswordEntry::new();
    client_secret.add_css_class("field-input");
    let port = gtk::Entry::new();
    port.set_placeholder_text(Some("e.g. 8765 (empty: any free port)"));
    port.add_css_class("field-input");
    let scopes = gtk::Entry::new();
    scopes.set_placeholder_text(Some("empty: what the server suggests"));
    scopes.add_css_class("field-input");
    let advanced = vbox(10);
    advanced.append(&crate::field("Environment variables, one per line (NAME=value)", &env));
    advanced.append(&crate::field("Headers for a remote server, one per line (Name: value)", &headers));
    advanced.append(&lbl("Sign-in. Most servers register ZO-AI by themselves. If one does not, register ZO-AI there with the redirect address http://127.0.0.1:PORT/callback (use the port below) and enter the client id.", "field-hint"));
    advanced.append(&crate::field("Client id", &client_id));
    advanced.append(&crate::field("Client secret (if it has one)", &client_secret));
    advanced.append(&crate::field("Local port for the sign-in", &port));
    advanced.append(&crate::field("Scopes (permissions to ask for)", &scopes));
    let reveal = gtk::Revealer::new();
    reveal.set_child(Some(&advanced));
    reveal.set_reveal_child(false);
    let more = secondary("More options");
    {
        let reveal = reveal.clone();
        more.connect_clicked(move |b| {
            let open = !reveal.reveals_child();
            reveal.set_reveal_child(open);
            b.set_label(if open { "Fewer options" } else { "More options" });
        });
    }
    body.append(&more);
    body.append(&reveal);
    let status = lbl("A server's address (https://…) connects to it; a command starts it here. It is tried once before it is saved. If the server asks you to sign in, your browser opens.", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);
    let (links, show_link) = link_row();
    body.append(&links);
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
            let text_of = |v: &gtk::TextView| {
                let b = v.buffer();
                b.text(&b.start_iter(), &b.end_iter(), false).to_string()
            };
            let pairs = |t: String, sep: char| -> std::collections::BTreeMap<String, String> {
                t.lines().filter_map(|l| l.split_once(sep)).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).filter(|(k, _)| !k.is_empty()).collect()
            };
            let nonempty = |e: &str| Some(e.trim().to_string()).filter(|t| !t.is_empty());
            let cfg = ServerConfig {
                name: mcp_client::clean_name(&name.text()),
                command: command.text().trim().to_string(),
                enabled: true,
                env: pairs(text_of(&env), '='),
                headers: pairs(text_of(&headers), ':'),
                client_id: nonempty(&client_id.text()),
                client_secret: nonempty(&client_secret.text()),
                callback_port: port.text().trim().parse::<u16>().ok(),
                scope: nonempty(&scopes.text()),
            };
            let typed_token = token.text().to_string();
            if cfg.name.is_empty() || cfg.command.is_empty() {
                status.set_text("Give it a name and an address or command.");
                return;
            }
            if mcp_client::load_servers().iter().any(|s| s.name == cfg.name) {
                status.set_text("A server with that name is already added.");
                return;
            }
            status.set_text(if mcp_client::is_url(&cfg.command) { "Connecting… if it asks you to sign in, your browser opens." } else { "Starting it…" });
            b.set_sensitive(false);
            let (tx, rx) = mpsc::channel();
            let (url_tx, url_rx) = mpsc::channel::<String>();
            let cfg2 = cfg.clone();
            std::thread::spawn(move || {
                // the sign-in page opens in the chosen browser, and its link is also handed to the window so it can be copied
                let open = move |u: &str| {
                    let _ = url_tx.send(u.to_string());
                    mcp_client::open_in_browser(u);
                };
                let _ = tx.send(mcp_client::connect(&cfg2, Some(&typed_token), &open));
            });
            let (status, b, w, ui3, show_link) = (status.clone(), b.clone(), w.clone(), ui2.clone(), show_link.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
                while let Ok(u) = url_rx.try_recv() {
                    show_link(&u);
                }
                match rx.try_recv() {
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
                }
            });
        });
    }
    win.present();
}

/// "Paste a config": the JSON other apps use (Claude Desktop, Claude Code, Cursor, VS Code). Every server in it is tried and added.
fn import_servers_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = crate::sheet(ui, "Paste an MCP configuration", 620);
    body.append(&lbl("Paste the JSON from a server's instructions, or from another app's configuration (Claude Desktop's claude_desktop_config.json, for example). Each server in it is tried, and the ones that work are added.", "field-hint"));
    let view = gtk::TextView::new();
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.add_css_class("field-input");
    view.set_size_request(-1, 200);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&view));
    scroll.set_min_content_height(200);
    body.append(&scroll);
    let status = lbl("", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);
    let (links, show_link) = link_row();
    body.append(&links);
    let cancel = secondary("Close");
    let go = crate::primary("Add the servers");
    foot.append(&cancel);
    foot.append(&go);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    {
        let (ui2, win2) = (ui.clone(), win.clone());
        go.connect_clicked(move |b| {
            let buf = view.buffer();
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            let found = match mcp_client::import_json(&text) {
                Ok(f) => f,
                Err(e) => {
                    status.set_text(&e);
                    return;
                }
            };
            let have: Vec<String> = mcp_client::load_servers().into_iter().map(|s| s.name).collect();
            let (new, old): (Vec<_>, Vec<_>) = found.into_iter().partition(|c| !have.contains(&c.name));
            if new.is_empty() {
                status.set_text(&format!("Already added: {}.", old.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join(", ")));
                return;
            }
            status.set_text(&format!("Trying {} server(s)… a server that needs a sign-in opens your browser.", new.len()));
            b.set_sensitive(false);
            let (tx, rx) = mpsc::channel();
            let (url_tx, url_rx) = mpsc::channel::<String>();
            std::thread::spawn(move || {
                let open = move |u: &str| {
                    let _ = url_tx.send(u.to_string());
                    mcp_client::open_in_browser(u);
                };
                let mut report = Vec::new();
                for cfg in new {
                    match mcp_client::connect(&cfg, None, &open) {
                        Ok(tools) => {
                            let mut all = mcp_client::load_servers();
                            all.push(cfg.clone());
                            mcp_client::save_servers(&all);
                            report.push(format!("✓ {}: {} tools", cfg.name, tools.len()));
                        }
                        Err(e) => report.push(format!("✗ {}: {e}", cfg.name)),
                    }
                }
                let _ = tx.send(report);
            });
            let (status, b, ui3, win3, show_link) = (status.clone(), b.clone(), ui2.clone(), win2.clone(), show_link.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                while let Ok(u) = url_rx.try_recv() {
                    show_link(&u);
                }
                match rx.try_recv() {
                    Ok(report) => {
                        status.set_text(&report.join("\n"));
                        b.set_sensitive(true);
                        show(&ui3);
                        let _ = &win3;
                        gtk::glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                    Err(_) => {
                        b.set_sensitive(true);
                        gtk::glib::ControlFlow::Break
                    }
                }
            });
        });
    }
    win.present();
}

/// A row that shows the sign-in link a server gave, with Copy and Open, so it can go to any browser or profile. Hidden until a link arrives.
fn link_row() -> (gtk::Box, Rc<dyn Fn(&str)>) {
    let row = hbox(8);
    row.set_visible(false);
    let label = lbl("", "field-hint");
    label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_selectable(true);
    let copy = secondary("Copy link");
    let open = secondary("Open");
    row.append(&label);
    row.append(&copy);
    row.append(&open);
    let url = Rc::new(std::cell::RefCell::new(String::new()));
    {
        let url = url.clone();
        copy.connect_clicked(move |b| b.clipboard().set_text(&url.borrow()));
    }
    {
        let url = url.clone();
        open.connect_clicked(move |_| mcp_client::open_in_browser(&url.borrow()));
    }
    let show = {
        let (row, label) = (row.clone(), label.clone());
        Rc::new(move |u: &str| {
            *url.borrow_mut() = u.to_string();
            label.set_text(&format!("Sign-in link: {u}"));
            row.set_visible(true);
        })
    };
    (row, show)
}
