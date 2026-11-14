//! The Settings page: the model and its key, the permission mode, how other apps connect to Zohara Agent, and the saved data.

use crate::{chat, chats, hbox, lbl, secondary, set_nav, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::rc::Rc;
use zo_core::ai::{agent, claude_code, profiles, providers, Selection};
use zo_core::mcp_client::{self, ServerConfig};
use std::sync::mpsc;

pub(crate) fn section(title: &str) -> (gtk::Box, gtk::Box) {
    let outer = vbox(8);
    outer.set_margin_bottom(26);
    outer.append(&lbl(title, "settings-head"));
    let card = vbox(0);
    card.add_css_class("settings-card");
    outer.append(&card);
    (outer, card)
}

/// One line in a section: a title and a few words under it on the left, a control on the right.
pub(crate) fn row(title: &str, sub: &str, control: Option<&impl IsA<gtk::Widget>>) -> gtk::Box {
    let r = hbox(14);
    r.add_css_class("settings-row");
    let col = vbox(3);
    col.set_hexpand(true);
    col.set_valign(gtk::Align::Center);
    if !title.is_empty() {
        col.append(&lbl(title, "pc-name"));
    }
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

/// The small text under a row's title (so it can be changed later).
fn find_sub(r: &gtk::Box) -> gtk::Label {
    let mut w = r.first_child();
    while let Some(c) = w {
        if let Some(col) = c.downcast_ref::<gtk::Box>() {
            let mut x = col.first_child();
            while let Some(l) = x {
                if let Some(label) = l.downcast_ref::<gtk::Label>() {
                    if label.has_css_class("pc-meta") {
                        return label.clone();
                    }
                }
                x = l.next_sibling();
            }
        }
        w = c.next_sibling();
    }
    lbl("", "pc-meta")
}

pub(crate) fn no_control() -> Option<&'static gtk::Button> {
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

thread_local! {
    /// Which Settings page is open: "" is the list of pages.
    static PAGE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

pub fn home(ui: &Rc<Ui>) {
    go(ui, "");
}

/// For verification: opens one Settings page straight away.
pub(crate) fn open_page(ui: &Rc<Ui>, page: &str) {
    go(ui, page);
}

fn go(ui: &Rc<Ui>, page: &str) {
    PAGE.with(|p| *p.borrow_mut() = page.to_string());
    show(ui);
}

/// Builds the page again from what is true now, and shows it.
pub fn show(ui: &Rc<Ui>) {
    let content = &ui.settings_box;
    while let Some(c) = content.first_child() {
        content.remove(&c);
    }
    content.set_spacing(0);
    let page = PAGE.with(|p| p.borrow().clone());
    let pages: [(&str, &str, &str); 9] = [
        ("model", "Model and profiles", "Which AI answers, its key, and saved presets."),
        ("personality", "Personality", "How Zohara Agent talks to you."),
        ("memory", "Memory", "What Zohara Agent remembers about you between chats."),
        ("general", "Appearance and permissions", "Light or dark, and what Zohara Agent may do on its own."),
        ("apps", "Connected apps", "Other programs that use Zohara Agent, and tools Zohara Agent can use."),
        ("sync", "Account and sync", "Optional: sign in to keep chats, memory and personalities in step with the web version."),
        ("machines", "Machines", "Other computers Zohara Agent can work on over SSH."),
        ("data", "Data", "Saved chats."),
        ("about", "About", "Version and updates."),
    ];
    if page.is_empty() || !pages.iter().any(|p| p.0 == page) {
        content.append(&lbl("Settings", "page-title"));
        if let Some(t) = content.first_child() {
            t.set_margin_bottom(24);
        }
        let card = vbox(0);
        card.add_css_class("settings-card");
        for (id, name, sub) in pages {
            let b = secondary("Open  ›");
            let (ui2, id) = (ui.clone(), id.to_string());
            b.connect_clicked(move |_| go(&ui2, &id));
            card.append(&row(name, sub, Some(&b)));
        }
        content.append(&card);
        ui.stack.set_visible_child_name("settings");
        set_nav(ui, "settings");
        ui.top_title.set_text("Settings");
        return;
    }
    {
        let bar = hbox(10);
        let back = secondary("‹ Settings");
        let ui2 = ui.clone();
        back.connect_clicked(move |_| go(&ui2, ""));
        bar.append(&back);
        let name = pages.iter().find(|p| p.0 == page).map(|p| p.1).unwrap_or("");
        bar.append(&lbl(name, "page-title"));
        bar.set_margin_bottom(24);
        content.append(&bar);
    }

    // ---- the model ----
    if page == "model" {
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
    // Claude Code: the state of the login on this computer, with Log in (a sheet with the sign-in link) and Log out
    {
        let cc_row = row("Claude Code", "Checking…", no_control());
        let state = find_sub(&cc_row);
        let ui2 = ui.clone();
        let (bar, refresh) = crate::claude_ui::buttons(ui, &state, move || show(&ui2));
        cc_row.append(&bar);
        refresh();
        card.append(&cc_row);
    }
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
            let pins = if p.pinned.is_empty() { String::new() } else { format!(" · {} pinned", p.pinned.len()) };
            card.append(&row(&p.name, &format!("{}{pins}", chat::describe(&p)), Some(&controls)));
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
        let endpoint = secondary("Add endpoint");
        {
            let ui2 = ui.clone();
            endpoint.connect_clicked(move |_| {
                let ui3 = ui2.clone();
                crate::modelpick::add_endpoint_sheet(&ui2, move || show(&ui3));
            });
        }
        card.append(&row("Add a custom endpoint", "Your own server or a gateway that speaks the OpenAI API: a name, its address, a key and a model. It is saved as a profile.", Some(&endpoint)));
    }
    content.append(&sec);

    // ---- every pinned model, with a way to take it back out ----
    let (sec, card) = section("Pinned models");
    {
        let mut any = false;
        for p in profiles::load() {
            let provider_name = providers::find(&p.provider).map(|x| x.name).unwrap_or("?");
            for model in &p.pinned {
                any = true;
                let un = secondary("Unpin");
                let (ui2, id, m) = (ui.clone(), p.id.clone(), model.clone());
                un.connect_clicked(move |_| {
                    profiles::unpin(&id, &m);
                    show(&ui2);
                });
                card.append(&row(&chat::nice_name(model), &format!("{} · {provider_name}", p.name), Some(&un)));
            }
        }
        if !any {
            card.append(&row("Nothing pinned yet", "Open the model list (Change above, or More models in the model menu) and press the star beside a model. Pinned models show first in the model menu, one click away.", no_control()));
        }
    }
    content.append(&sec);

    // ---- which providers show in the pickers ----
    let (sec, card) = section("Providers in the model picker");
    card.append(&row("", "Switch off the providers you never use so the pickers stay short. The one in use always shows. This choice follows you to your other devices when sync is on.", no_control()));
    {
        let hidden = zo_core::ai::ui_prefs::hidden_providers();
        let mut status_labels: Vec<(String, gtk::Label)> = Vec::new();
        for p in providers::all() {
            let sw = gtk::Switch::new();
            sw.set_active(!hidden.iter().any(|h| h == p.id));
            sw.set_tooltip_text(Some("Show in the model picker"));
            let id = p.id.to_string();
            sw.connect_active_notify(move |s| zo_core::ai::ui_prefs::set_provider_hidden(&id, !s.is_active()));
            let r = row(p.name, "Checking the key…", Some(&sw));
            if let Some(l) = r.first_child().and_then(|c| c.last_child()).and_then(|l| l.downcast::<gtk::Label>().ok()) {
                status_labels.push((p.id.to_string(), l));
            }
            card.append(&r);
        }
        // key status is asked of the keyring in a worker thread, so the page opens at once
        gtk::glib::spawn_future_local(async move {
            let found = gtk::gio::spawn_blocking(|| {
                let profs = profiles::load();
                providers::all()
                    .iter()
                    .map(|p| {
                        let pins: usize = profs.iter().filter(|x| x.provider == p.id).map(|x| x.pinned.len()).sum();
                        let state = if p.protocol == providers::Protocol::ClaudeCode {
                            "Uses your Claude Code sign-in".to_string()
                        } else if p.local {
                            "Runs on this computer, no key".to_string()
                        } else if zo_core::ai::keys::get(p.id).is_some() || profs.iter().any(|x| x.provider == p.id && zo_core::ai::keys::get(&format!("{}@{}", p.id, x.id)).is_some()) {
                            "Key saved".to_string()
                        } else {
                            "No key yet".to_string()
                        };
                        (p.id.to_string(), if pins > 0 { format!("{state} · {pins} pinned") } else { state })
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            for (id, text) in found {
                if let Some((_, l)) = status_labels.iter().find(|(i, _)| *i == id) {
                    l.set_text(&text);
                }
            }
        });
    }
    content.append(&sec);
    }

    // ---- looks ----
    if page == "general" {
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
        "Manual asks before every change and command. Auto lets looking around run and asks before changes. Bypass never asks. The same rule applies to every app connected to Zohara Agent.",
        Some(&dd),
    ));
    content.append(&sec);

    // ---- tray and startup ----
    let (sec, card) = section("Tray and startup");
    let present = crate::tray::present();
    let keep = gtk::Switch::new();
    keep.set_active(crate::tray::keep_setting().unwrap_or(true));
    keep.connect_state_set(|_, on| {
        crate::tray::set_keep(on);
        gtk::glib::Propagation::Proceed
    });
    let sub = if present {
        "Closing the window leaves Zohara Agent running (chats, tasks, the phone link) with its icon in the panel, where you can open it, see what is running and quit."
    } else {
        "No panel that shows tray icons was found, so closing the window quits Zohara Agent (it asks first when something is still running). This takes effect when a tray appears."
    };
    card.append(&row("Keep running in the tray when the window is closed", sub, Some(&keep)));
    let auto = gtk::Switch::new();
    auto.set_active(crate::tray::autostart_enabled());
    let auto_note = wrapped("");
    auto_note.set_margin_start(16);
    auto_note.set_margin_end(16);
    auto_note.set_margin_bottom(10);
    auto_note.set_visible(false);
    {
        let note = auto_note.clone();
        auto.connect_state_set(move |_, on| {
            let text = match crate::tray::set_autostart(on) {
                Ok(()) => String::new(),
                Err(e) => format!("Could not change this: {e}"),
            };
            note.set_visible(!text.is_empty());
            note.set_text(&text);
            gtk::glib::Propagation::Proceed
        });
    }
    card.append(&row("Start with the computer", "Zohara Agent starts without a window when you sign in, so the tray icon, the shortcut and the phone link are ready.", Some(&auto)));
    card.append(&auto_note);
    content.append(&sec);
    }

    // ---- other apps ----
    if page == "apps" {
    let (sec, card) = section("Connect other apps");
    let cmd = format!("claude mcp add zohara -- {}", zo_mcp_path());
    let copy = secondary("Copy");
    let c2 = cmd.clone();
    copy.connect_clicked(move |b| b.clipboard().set_text(&c2));
    card.append(&row("Let Claude Code control this computer", "Any app that speaks MCP can use Zohara Agent's tools (files, shell, screenshots, schedules, projects). Run this once:", Some(&copy)));
    let code = lbl(&cmd, "settings-code");
    code.set_selectable(true);
    code.set_wrap(true);
    code.set_margin_start(16);
    code.set_margin_end(16);
    code.set_margin_bottom(14);
    card.append(&code);
    content.append(&sec);

    // ---- MCP servers Zohara Agent can use ----
    let (sec, card) = section("MCP servers Zohara Agent can use");
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
        sw.set_active(s.enabled && !s.held);
        sw.set_valign(gtk::Align::Center);
        let name = s.name.clone();
        sw.connect_state_set(move |_, on| {
            let mut all = mcp_client::load_servers();
            if let Some(x) = all.iter_mut().find(|x| x.name == name) {
                x.enabled = on;
                x.held = false;
            }
            mcp_client::save_servers(&all);
            gtk::glib::Propagation::Proceed
        });
        r.append(&sw);
        let del = crate::icon_button("edit-delete-symbolic", "Remove this server", "icon-btn");
        let (ui2, name) = (ui.clone(), s.name.clone());
        del.connect_clicked(move |_| {
            let (ui3, name3) = (ui2.clone(), name.clone());
            crate::confirm_delete(&ui2, &format!("Remove \"{name}\"?"), "Zohara Agent stops using this server and forgets its sign-in.", move || {
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
    }

    // ---- machines: other computers over SSH ----
    if page == "machines" {
        let (sec, card) = section("Machines");
        let add = secondary("Add machine");
        let ui2 = ui.clone();
        add.connect_clicked(move |_| add_machine_sheet(&ui2));
        card.append(&row(
            "Let Zohara Agent work on another computer",
            "A cloud VM or a server you can reach with SSH. Zohara Agent can run commands there and read and write files, asking you first unless the mode is Bypass. The private key is kept in the system keyring.",
            Some(&add),
        ));
        for m in zo_core::store::machines::load() {
            let r = hbox(12);
            r.add_css_class("settings-row");
            let col = vbox(3);
            col.set_hexpand(true);
            col.append(&lbl(&m.name, "pc-name"));
            let mut meta = format!("{}@{}:{}", m.user, m.host, m.port);
            if !m.note.is_empty() {
                meta.push_str(&format!("  ·  {}", m.note));
            }
            if !m.has_key() {
                meta.push_str("  ·  no key saved");
            }
            let meta = lbl(&meta, "pc-meta");
            meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
            col.append(&meta);
            let status = lbl("", "field-hint");
            status.set_wrap(true);
            status.set_xalign(0.0);
            status.set_max_width_chars(60);
            col.append(&status);
            r.append(&col);
            let test = secondary("Test");
            {
                let (name, status) = (m.name.clone(), status.clone());
                test.connect_clicked(move |b| {
                    b.set_sensitive(false);
                    status.set_text("Connecting…");
                    let (tx, rx) = mpsc::channel();
                    let n = name.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(zo_core::tools::ssh::test(&n));
                    });
                    let (b, status) = (b.clone(), status.clone());
                    gtk::glib::timeout_add_local(std::time::Duration::from_millis(150), move || match rx.try_recv() {
                        Ok(r) => {
                            status.set_text(&r.unwrap_or_else(|e| e));
                            b.set_sensitive(true);
                            gtk::glib::ControlFlow::Break
                        }
                        Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                        Err(_) => gtk::glib::ControlFlow::Break,
                    });
                });
            }
            r.append(&test);
            let del = crate::icon_button("edit-delete-symbolic", "Remove this machine", "icon-btn");
            let (ui2, name) = (ui.clone(), m.name.clone());
            del.connect_clicked(move |_| {
                let (ui3, name3) = (ui2.clone(), name.clone());
                crate::confirm_delete(&ui2, &format!("Remove \"{name}\"?"), "Zohara Agent forgets this machine and its private key. Nothing changes on the machine itself.", move || {
                    let _ = zo_core::store::machines::remove(&name3);
                    show(&ui3);
                });
            });
            r.append(&del);
            card.append(&r);
        }
        content.append(&sec);
    }

    // ---- personality: how Zohara Agent talks ----
    if page == "personality" {
        use zo_core::store::personality as pers;
        let (sec, card) = section("Personality");
        let st = pers::load();
        let plain = hbox(6);
        let plain_btn = if st.active.is_empty() {
            let b = secondary("In use");
            b.set_sensitive(false);
            b
        } else {
            crate::primary("Use")
        };
        {
            let ui2 = ui.clone();
            plain_btn.connect_clicked(move |_| {
                let _ = pers::set_active("");
                show(&ui2);
            });
        }
        plain.append(&plain_btn);
        card.append(&row("Plain", "Zohara Agent's own manner: clear and to the point.", Some(&plain)));
        let builtin_ids: Vec<String> = pers::builtin().into_iter().map(|b| b.id).collect();
        for p in pers::all() {
            let active = st.active == p.id;
            let controls = hbox(6);
            let b = if active {
                let b = secondary("In use");
                b.set_sensitive(false);
                b
            } else {
                crate::primary("Use")
            };
            {
                let (ui2, id) = (ui.clone(), p.id.clone());
                b.connect_clicked(move |_| {
                    let _ = pers::set_active(&id);
                    show(&ui2);
                });
            }
            controls.append(&b);
            if !builtin_ids.contains(&p.id) {
                let del = crate::icon_button("edit-delete-symbolic", "Delete this personality", "icon-btn");
                let (ui2, id) = (ui.clone(), p.id.clone());
                del.connect_clicked(move |_| {
                    pers::remove(&id);
                    show(&ui2);
                });
                controls.append(&del);
            }
            let tag = if p.generated { " · written by the AI" } else { "" };
            card.append(&row(&format!("{}{}", p.name, tag), &p.prompt, Some(&controls)));
        }
        let make = crate::primary("Describe one…");
        {
            let ui2 = ui.clone();
            make.connect_clicked(move |_| {
                let ui3 = ui2.clone();
                text_sheet(&ui2, "Make a personality", "", "What should it be like?", "A patient teacher who explains with small everyday examples and never rushes me", "Write it", true, |d| {
                    zo_core::ai::personality_gen::generate(&d).map(|p| p.name)
                }, move || show(&ui3));
            });
        }
        let own = secondary("Write my own…");
        {
            let ui2 = ui.clone();
            own.connect_clicked(move |_| {
                let ui3 = ui2.clone();
                text_sheet(&ui2, "Write a personality", "", "How should it sound? (4 to 7 sentences)", "Sound like …", "Save", false, |t| {
                    let name: String = t.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
                    zo_core::store::personality::add(&name, &t, false).map(|p| p.name)
                }, move || show(&ui3));
            });
        }
        let both = hbox(6);
        both.append(&own);
        both.append(&make);
        card.append(&row("Add a personality", "The AI can write one from a description (it uses the model you chose). A personality changes the voice only, never the rules or what Zohara Agent may do.", Some(&both)));
        content.append(&sec);
    }

    // ---- memory: what Zohara Agent remembers about you ----
    if page == "memory" {
        use zo_core::store::memory as mem;
        let (sec, card) = section("Memory");
        let st = mem::load();
        let sw = gtk::Switch::new();
        sw.set_active(st.enabled);
        sw.connect_state_set(|_, on| {
            mem::set_enabled(on);
            gtk::glib::Propagation::Proceed
        });
        let notes = st.items.iter().filter(|i| i.kind != "episode").count();
        let chats_n = st.items.len() - notes;
        card.append(&row(
            "Remember things between chats",
            &format!("{notes} notes about you and {chats_n} chat summaries, shared with the phone. After every {} messages of yours, Zohara Agent reads the chat in the background and keeps what is worth remembering. Never passwords or keys.", st.every),
            Some(&sw),
        ));
        let talk = crate::primary("Chat about memory…");
        {
            let ui2 = ui.clone();
            talk.connect_clicked(move |_| memory_chat_sheet(&ui2));
        }
        card.append(&row("Edit by talking", "Tell Zohara Agent what to change, in your own words: \"join my preferences into one note\", \"forget the old laptop\", \"my name is spelled Zohaib\". This chat only edits the notes.", Some(&talk)));
        let mut items = st.items.clone();
        items.sort_by(|a, b| (b.pinned, b.updated).cmp(&(a.pinned, a.updated)));
        let groups = [("person", "About you"), ("preference", "Preferences"), ("project", "Projects"), ("fact", "Facts"), ("episode", "Chat summaries")];
        for (kind, label) in groups {
            let mine: Vec<_> = items.iter().filter(|i| i.kind == kind).collect();
            if mine.is_empty() {
                continue;
            }
            let h = lbl(label, "pc-meta");
            h.set_xalign(0.0);
            h.set_margin_top(14);
            h.set_margin_start(16);
            h.set_margin_bottom(4);
            card.append(&h);
            for it in mine.into_iter().take(80) {
                let controls = hbox(6);
                if kind != "episode" {
                    let edit = crate::icon_button("document-edit-symbolic", "Edit this note", "icon-btn");
                    let (ui2, id, text) = (ui.clone(), it.id.clone(), it.text.clone());
                    edit.connect_clicked(move |_| {
                        let ui3 = ui2.clone();
                        let id = id.clone();
                        text_sheet(&ui2, "Edit note", &text, "What should Zohara Agent remember?", "I prefer short answers", "Save", false, move |t| {
                            if mem::update_text(&id, &t) { Ok("Saved".to_string()) } else { Err("That note is too short.".to_string()) }
                        }, move || show(&ui3));
                    });
                    controls.append(&edit);
                }
                let pin = crate::icon_button(if it.pinned { "starred-symbolic" } else { "non-starred-symbolic" }, if it.pinned { "Always tell the AI (pinned)" } else { "Pin: always tell the AI" }, "icon-btn");
                {
                    let (ui2, id, on) = (ui.clone(), it.id.clone(), !it.pinned);
                    pin.connect_clicked(move |_| {
                        mem::set_pinned(&id, on);
                        show(&ui2);
                    });
                }
                controls.append(&pin);
                let del = crate::icon_button("edit-delete-symbolic", "Forget this", "icon-btn");
                {
                    let (ui2, id) = (ui.clone(), it.id.clone());
                    del.connect_clicked(move |_| {
                        mem::forget(&id);
                        show(&ui2);
                    });
                }
                controls.append(&del);
                let title = if kind == "episode" { it.title.clone() } else { String::new() };
                card.append(&row(&title, &it.text, Some(&controls)));
            }
        }
        if items.len() > 80 {
            card.append(&row("…", &format!("{} more are kept; the AI can search all of them.", items.len() - 80), no_control()));
        }
        let add = secondary("Add a note…");
        {
            let ui2 = ui.clone();
            add.connect_clicked(move |_| {
                let ui3 = ui2.clone();
                text_sheet(&ui2, "Add a note", "", "What should Zohara Agent remember?", "I prefer short answers", "Add", false, |t| {
                    mem::remember(&t, "fact", "").map(|_| "Remembered".to_string()).ok_or_else(|| "That note is too short.".to_string())
                }, move || show(&ui3));
            });
        }
        let forget = secondary("Forget everything");
        forget.add_css_class("danger");
        {
            let ui2 = ui.clone();
            forget.connect_clicked(move |_| {
                let ui3 = ui2.clone();
                crate::confirm_delete(&ui2, "Forget everything?", "All notes and chat summaries are removed, on the computer and for the phone. Saved chats stay.", move || {
                    mem::clear_all();
                    show(&ui3);
                });
            });
        }
        let both = hbox(6);
        both.append(&add);
        both.append(&forget);
        card.append(&row("Your notes", "Pin a note to make sure the AI always knows it. Delete one that is wrong. You can also just tell Zohara Agent: \"remember that ...\" or \"forget that ...\".", Some(&both)));
        content.append(&sec);
    }

    // ---- data ----
    if page == "data" {
    let (sec, card) = section("Data");
    let n = chats::load().len();
    let open = secondary("View chats");
    {
        let ui2 = ui.clone();
        open.connect_clicked(move |_| saved_chats_sheet(&ui2));
    }
    card.append(&row("Saved chats, projects and tasks", &format!("{n} saved chats, kept on this computer. Open the list to read them here."), Some(&open)));
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
    }

    // ---- account and sync ----
    if page == "sync" {
        account_page(ui, content);
    }

    // ---- about ----
    if page == "about" {
        let (sec, card) = section("About");
        card.append(&row("Zohara Agent", &format!("Version {} · the AI built into Zohara OS", zo_core::update::VERSION), no_control()));
        let _ = agent::SYSTEM_PROMPT;
        content.append(&sec);
        crate::updates::about_updates(ui, content);
    }

    ui.stack.set_visible_child_name("settings");
    set_nav(ui, "settings");
    ui.top_title.set_text("Settings");
}

/// The sync status as one line for the Account and sync page.
fn sync_line() -> String {
    let s = zo_core::sync::status();
    let when = |ms: u64| {
        let ago = zo_core::store::chats::now_millis().saturating_sub(ms) / 1000;
        match ago {
            0..=59 => "just now".to_string(),
            60..=3599 => format!("{} min ago", ago / 60),
            _ => format!("{} h ago", ago / 3600),
        }
    };
    let counts = format!("{} received, {} sent, {} deleted", s.pulled, s.pushed, s.deleted);
    match (s.last_ok, s.error.is_empty()) {
        (0, true) => "Not yet synced.".to_string(),
        (0, false) => format!("The last try failed: {}", s.error),
        (t, true) => format!("Last sync {} ({counts}).", when(t)),
        (t, false) => format!("The last try failed: {} Last good sync {} ({counts}).", s.error, when(t)),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// Account and sync (optional sign-in; nothing is contacted before the person chooses it)

thread_local! {
    /// A message for the next time the Account page is built (for instance "the server could not be told about the sign-out").
    static NOTICE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Runs `work` off the main thread and hands its result to `done` on the main thread.
pub(crate) fn background<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    let mut done = Some(done);
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(150), move || match rx.try_recv() {
        Ok(v) => {
            if let Some(d) = done.take() {
                d(v);
            }
            gtk::glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
        Err(_) => gtk::glib::ControlFlow::Break,
    });
}

pub(crate) fn wrapped(text: &str) -> gtk::Label {
    let l = lbl(text, "field-hint");
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

fn padded_box() -> gtk::Box {
    let b = vbox(12);
    b.set_margin_top(14);
    b.set_margin_bottom(14);
    b.set_margin_start(16);
    b.set_margin_end(16);
    b
}

fn account_page(ui: &Rc<Ui>, content: &gtk::Box) {
    use zo_core::{login, sync};
    let notice = NOTICE.with(|n| std::mem::take(&mut *n.borrow_mut()));
    let signed_in = login::signed_in_quick();
    let cfg = sync::config();
    match &signed_in {
        Some(email) => {
            let (sec, card) = section("Account");
            let out = secondary("Sign out");
            card.append(&row("Signed in", email, Some(&out)));
            let st = wrapped(&notice);
            st.set_margin_start(16);
            st.set_margin_end(16);
            st.set_margin_bottom(if notice.is_empty() { 0 } else { 12 });
            card.append(&st);
            {
                let ui2 = ui.clone();
                out.connect_clicked(move |_| {
                    let dialog = adw::AlertDialog::new(Some("Sign out?"), Some("This computer stops syncing and forgets the sign-in. Your chats, memory and personalities stay here and on the server."));
                    dialog.add_responses(&[("cancel", "Cancel"), ("out", "Sign out")]);
                    dialog.set_response_appearance("out", adw::ResponseAppearance::Destructive);
                    dialog.set_default_response(Some("cancel"));
                    dialog.set_close_response("cancel");
                    let ui3 = ui2.clone();
                    dialog.connect_response(None, move |_, id| {
                        if id == "out" {
                            let ui4 = ui3.clone();
                            background(zo_core::login::sign_out, move |r| {
                                if let Err(e) = r {
                                    NOTICE.with(|n| *n.borrow_mut() = e);
                                }
                                if PAGE.with(|p| p.borrow().as_str() == "sync") {
                                    show(&ui4);
                                }
                            });
                        }
                    });
                    dialog.present(Some(&ui2.window));
                });
            }
            content.append(&sec);

            let (sec, card) = section("Sync");
            let on = gtk::Switch::new();
            on.set_active(cfg.enabled);
            let status = wrapped("");
            let now = secondary("Sync now");
            let bar = hbox(8);
            bar.set_margin_start(16);
            bar.set_margin_end(16);
            bar.set_margin_bottom(14);
            bar.set_margin_top(4);
            bar.append(&now);
            {
                let status = status.clone();
                on.connect_state_set(move |_, state| {
                    status.set_text(&match sync::save_config(&sync::config().url, state) {
                        Ok(()) => sync_line(),
                        Err(e) => e,
                    });
                    gtk::glib::Propagation::Proceed
                });
            }
            {
                let status = status.clone();
                now.connect_clicked(move |b| {
                    b.set_sensitive(false);
                    status.set_text("Syncing…");
                    let (b, status) = (b.clone(), status.clone());
                    background(sync::sync_now, move |_| {
                        b.set_sensitive(true);
                        status.set_text(&sync_line());
                    });
                });
            }
            card.append(&row("Keep in sync", "Every 30 seconds this computer and the server compare chats, memory notes and personalities. The newest change wins; a deleted chat or note is deleted on both. API keys are not sent unless you switch that on below.", Some(&on)));
            // what else travels: profiles and pinned models, remote MCP servers, and (off by default) API keys
            let (sp, sm, sk) = (gtk::Switch::new(), gtk::Switch::new(), gtk::Switch::new());
            sp.set_active(cfg.profiles);
            sm.set_active(cfg.mcp);
            sk.set_active(cfg.keys);
            for sw in [&sp, &sm, &sk] {
                let (sp, sm, sk, status) = (sp.clone(), sm.clone(), sk.clone(), status.clone());
                sw.connect_state_set(move |_, _| {
                    if let Err(e) = sync::set_kinds(sp.is_active(), sm.is_active(), sk.is_active()) {
                        status.set_text(&e);
                    }
                    gtk::glib::Propagation::Proceed
                });
            }
            card.append(&row("Sync my profiles and pinned models", "Your profiles (provider, model, address, effort), the models you pinned and which providers you hide in the pickers. No API key is in them.", Some(&sp)));
            card.append(&row("Sync MCP servers (web addresses only)", "MCP servers you added by web address, with their on/off switch. Programs that run on this computer, tokens and sign-ins stay here.", Some(&sm)));
            card.append(&row("Also sync my API keys", "Off by default. When on, your API keys are also sent to the server (over HTTPS, to your own account only) and kept there, encrypted with the server's secret, so the web version can use them. Anyone who runs the server could in principle read them. Switch it off and delete them on the web if you do not want that.", Some(&sk)));
            card.append(&bar);
            status.set_margin_start(16);
            status.set_margin_end(16);
            status.set_margin_bottom(14);
            status.set_text(&sync_line());
            card.append(&status);
            {
                // the status line follows the background sync while this page is open
                let status = status.clone();
                gtk::glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
                    if status.root().is_none() {
                        return gtk::glib::ControlFlow::Break;
                    }
                    let t = status.text();
                    if t.is_empty() || t.starts_with("Last") || t.starts_with("Not yet") || t.starts_with("The last try") {
                        status.set_text(&sync_line());
                    }
                    gtk::glib::ControlFlow::Continue
                });
            }
            content.append(&sec);

            link_sections(ui, content);
        }
        None => signed_out_cards(ui, content, &notice),
    }

    // for people who run their own server
    let (sec, card) = section("");
    let ex = gtk::Expander::new(Some("Advanced: server address"));
    let inner = padded_box();
    let url = gtk::Entry::new();
    url.add_css_class("field-input");
    url.set_placeholder_text(Some(login::DEFAULT_SERVER_URL));
    url.set_text(&cfg.url);
    url.set_hexpand(true);
    inner.append(&wrapped("Only for people who run their own Zohara Agent server. Leave it empty to use the Zohara Agent server. Sign out before changing it."));
    inner.append(&crate::field("Server address", &url));
    let save = secondary("Save address");
    save.set_halign(gtk::Align::Start);
    save.set_sensitive(signed_in.is_none());
    url.set_sensitive(signed_in.is_none());
    inner.append(&save);
    let st = wrapped("");
    inner.append(&st);
    ex.set_child(Some(&inner));
    ex.set_margin_top(10);
    ex.set_margin_bottom(10);
    ex.set_margin_start(16);
    ex.set_margin_end(16);
    card.append(&ex);
    save.connect_clicked(move |_| {
        st.set_text(&match sync::save_config(&url.text(), sync::config().enabled) {
            Ok(()) => "Saved.".to_string(),
            Err(e) => e,
        });
    });
    content.append(&sec);
}

enum Outcome {
    SignedIn,
    /// Created (a verification e-mail went out) or not verified yet; the ID token is kept for "Resend".
    NeedsVerification(zo_core::login::Session, bool),
    Failed(String),
}

fn signed_out_cards(ui: &Rc<Ui>, content: &gtk::Box, notice: &str) {
    use zo_core::login;
    let (sec, card) = section("Account");
    let box_ = padded_box();
    box_.append(&wrapped("Sign in to keep your chats, memory and personalities in step with the web version and your other computers. Optional: Zohara Agent works fully without it. Nothing is sent anywhere until you sign in."));
    let google = crate::primary("Continue with Google");
    google.set_halign(gtk::Align::Start);
    {
        let ui2 = ui.clone();
        google.connect_clicked(move |_| google_sheet(&ui2));
    }
    box_.append(&google);
    card.append(&box_);
    content.append(&sec);

    let (sec, card) = section("Or with e-mail and password");
    let f = padded_box();
    let email = gtk::Entry::new();
    email.add_css_class("field-input");
    email.set_input_purpose(gtk::InputPurpose::Email);
    email.set_hexpand(true);
    let password = gtk::PasswordEntry::new();
    password.set_show_peek_icon(true);
    password.add_css_class("field-input");
    password.set_hexpand(true);
    f.append(&crate::field("E-mail", &email));
    f.append(&crate::field("Password", &password));
    let sign_in = crate::primary("Sign in");
    let create = secondary("Create account");
    let forgot = gtk::Button::with_label("Forgot password?");
    forgot.add_css_class("flat");
    let buttons = hbox(8);
    buttons.append(&sign_in);
    buttons.append(&create);
    buttons.append(&forgot);
    f.append(&buttons);
    let status = wrapped(notice);
    f.append(&status);
    let resend = secondary("Resend verification e-mail");
    resend.set_halign(gtk::Align::Start);
    resend.set_visible(false);
    f.append(&resend);
    card.append(&f);
    content.append(&sec);

    let pending: Rc<std::cell::RefCell<Option<login::Session>>> = Rc::new(std::cell::RefCell::new(None));
    let all: Rc<Vec<gtk::Button>> = Rc::new(vec![sign_in.clone(), create.clone(), forgot.clone(), resend.clone()]);
    let go_ui = ui.clone();
    let run: Rc<dyn Fn(bool)> = {
        let (email, password, status, resend, pending, all) = (email.clone(), password.clone(), status.clone(), resend.clone(), pending.clone(), all.clone());
        Rc::new(move |sign_up: bool| {
            let (e, pw) = (email.text().to_string(), password.text().to_string());
            status.set_text(if sign_up { "Creating the account…" } else { "Signing in…" });
            resend.set_visible(false);
            all.iter().for_each(|b| b.set_sensitive(false));
            let (status, resend, pending, all, ui) = (status.clone(), resend.clone(), pending.clone(), all.clone(), go_ui.clone());
            background(
                move || -> Outcome {
                    let s = match if sign_up { login::sign_up(&e, &pw) } else { login::sign_in_password(&e, &pw) } {
                        Ok(s) => s,
                        Err(m) => return Outcome::Failed(m),
                    };
                    if sign_up || s.email_verified == Some(false) {
                        return Outcome::NeedsVerification(s.clone(), sign_up && s.verification_sent);
                    }
                    match login::exchange(&s.id_token, &login::default_device_name("desktop"), "desktop") {
                        Ok(l) => match login::save_login(&l) {
                            Ok(()) => Outcome::SignedIn,
                            Err(m) => Outcome::Failed(m),
                        },
                        Err(m) if m.to_lowercase().contains("verif") => Outcome::NeedsVerification(s, false),
                        Err(m) => Outcome::Failed(m),
                    }
                },
                move |o| {
                    all.iter().for_each(|b| b.set_sensitive(true));
                    match o {
                        Outcome::SignedIn => {
                            if PAGE.with(|p| p.borrow().as_str() == "sync") {
                                show(&ui);
                            }
                        }
                        Outcome::NeedsVerification(s, sent) => {
                            status.set_text(&verification_text(&s.email, sign_up, sent));
                            *pending.borrow_mut() = Some(s);
                            resend.set_visible(true);
                        }
                        Outcome::Failed(m) => status.set_text(&m),
                    }
                },
            );
        })
    };
    {
        let run = run.clone();
        sign_in.connect_clicked(move |_| run(false));
    }
    {
        let run = run.clone();
        password.connect_activate(move |_| run(false));
    }
    create.connect_clicked(move |_| run(true));
    {
        let (email, status, all) = (email.clone(), status.clone(), all.clone());
        forgot.connect_clicked(move |_| {
            let e = email.text().to_string();
            status.set_text("Sending…");
            all.iter().for_each(|b| b.set_sensitive(false));
            let (status, all) = (status.clone(), all.clone());
            let e2 = e.clone();
            background(move || login::reset_password(&e2), move |r| {
                all.iter().for_each(|b| b.set_sensitive(true));
                status.set_text(&match r {
                    Ok(()) => format!("If there is an account for {}, an e-mail to choose a new password is on its way.", e.trim()),
                    Err(m) => m,
                });
            });
        });
    }
    {
        let (status, pending, all) = (status.clone(), pending.clone(), all.clone());
        resend.connect_clicked(move |_| {
            let Some(s) = pending.borrow().clone() else { return };
            status.set_text("Sending…");
            all.iter().for_each(|b| b.set_sensitive(false));
            let (status, all) = (status.clone(), all.clone());
            background(move || login::send_verification(&s.id_token), move |r| {
                all.iter().for_each(|b| b.set_sensitive(true));
                status.set_text(&match r {
                    Ok(()) => "Sent again. Check your inbox (and the spam folder).".to_string(),
                    Err(m) => m,
                });
            });
        });
    }
}

/// What to tell the person when the e-mail address is not verified yet.
fn verification_text(email: &str, created: bool, sent: bool) -> String {
    let first = match (created, sent) {
        (true, true) => format!("Account created. We sent a verification e-mail to {email}."),
        (true, false) => format!("Account created, but the verification e-mail could not be sent to {email}. Press Resend."),
        (false, _) => format!("The e-mail address {email} is not verified yet."),
    };
    format!("{first} Open the link in it, then press Sign in.")
}

/// "Continue with Google": the system browser opens the approval page, and this panel shows the code to type there.
fn google_sheet(ui: &Rc<Ui>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use zo_core::login;
    enum Msg {
        Code(String, String),
        Done(Result<(), String>),
    }
    let (win, body, foot) = crate::sheet(ui, "Continue with Google", 460);
    let info = wrapped("Contacting the server…");
    let code = gtk::Label::new(None);
    code.set_selectable(true);
    code.set_margin_top(6);
    code.set_margin_bottom(6);
    let status = wrapped("");
    body.append(&info);
    body.append(&code);
    body.append(&status);
    let cancel_btn = secondary("Cancel");
    foot.append(&cancel_btn);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let (w, c) = (win.clone(), cancel.clone());
        cancel_btn.connect_clicked(move |_| {
            c.store(true, Ordering::SeqCst);
            w.close();
        });
    }
    let (tx, rx) = mpsc::channel();
    {
        let (tx, cancel) = (tx, cancel.clone());
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let r = login::run_device_flow(&login::default_device_name("desktop"), "desktop", move |c, u| {
                let _ = tx2.send(Msg::Code(c.to_string(), u.to_string()));
            }, &cancel)
            .and_then(|l| login::save_login(&l));
            let _ = tx.send(Msg::Done(r));
        });
    }
    let ui2 = ui.clone();
    let win2 = win.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if win2.scrim.parent().is_none() {
            // closed with X or Esc: stop waiting
            cancel.store(true, Ordering::SeqCst);
            return gtk::glib::ControlFlow::Break;
        }
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Code(c, u) => {
                    code.set_markup(&format!("<span size='xx-large' weight='bold' font_family='monospace'>{}</span>", gtk::glib::markup_escape_text(&c)));
                    code.set_halign(gtk::Align::Center);
                    info.set_text(&format!("Open {u} if the browser did not open, sign in, then type this code:"));
                    status.set_text("Waiting for you to approve this computer. Only approve if you started this yourself.");
                    if browser_url_ok(&u) {
                        gtk::UriLauncher::new(&u).launch(Some(&ui2.window), gtk::gio::Cancellable::NONE, |_| {});
                    }
                }
                Msg::Done(Ok(())) => {
                    win2.close();
                    if PAGE.with(|p| p.borrow().as_str() == "sync") {
                        show(&ui2);
                    }
                    return gtk::glib::ControlFlow::Break;
                }
                Msg::Done(Err(e)) => {
                    if e != "Cancelled." {
                        code.set_text("");
                        info.set_text("Signing in did not work.");
                        status.set_text(&e);
                        cancel_btn.set_label("Close");
                    }
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
        gtk::glib::ControlFlow::Continue
    });
    win.present();
}

/// The browser is only opened for an https address (or one on this computer, for a test server).
fn browser_url_ok(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://127.0.0.1") || u.starts_with("http://localhost")
}

// ---- the one-time card on the home screen ----

fn dismissed_flag() -> std::path::PathBuf {
    zo_core::audit::config_dir().join("signin-card-dismissed")
}

/// Whether the home screen's "Sign in" card is shown: not after signing in, not after "Not now".
pub(crate) fn card_visible(signed_in: bool, dismissed: bool) -> bool {
    !signed_in && !dismissed
}

pub(crate) fn signin_card_wanted() -> bool {
    card_visible(zo_core::login::signed_in_quick().is_some(), dismissed_flag().exists())
}

pub(crate) fn dismiss_signin_card() {
    let p = dismissed_flag();
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
    let _ = std::fs::write(&p, "");
}

/// A small panel with one text box. `work` gets the text and runs off the main thread (it may call the model); `done` runs on the
/// main thread when it succeeded. While it works the panel says so; an error stays on the panel.
fn text_sheet(ui: &Rc<Ui>, title: &str, initial: &str, label: &str, placeholder: &str, ok_text: &str, multi: bool, work: impl Fn(String) -> Result<String, String> + Send + Sync + 'static, done: impl Fn() + 'static) {
    let (win, body, foot) = crate::sheet(ui, title, 460);
    let view = gtk::TextView::new();
    view.buffer().set_text(initial);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.set_left_margin(10);
    view.set_right_margin(10);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&view));
    scroll.set_min_content_height(if multi { 96 } else { 56 });
    scroll.add_css_class("field-input");
    body.append(&crate::field(label, &scroll));
    body.append(&lbl(&format!("For example: {placeholder}"), "field-hint"));
    let status = lbl("", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);
    let cancel = secondary("Cancel");
    let ok = crate::primary(ok_text);
    foot.append(&cancel);
    foot.append(&ok);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    let work = std::sync::Arc::new(work);
    let done: Rc<dyn Fn()> = Rc::new(done);
    {
        let (w, view, status, okb) = (win.clone(), view.clone(), status.clone(), ok.clone());
        ok.connect_clicked(move |_| {
            let buf = view.buffer();
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            okb.set_sensitive(false);
            status.set_text("Working on it…");
            let (tx, rx) = mpsc::channel();
            let work = work.clone();
            std::thread::spawn(move || {
                let _ = tx.send(work(text));
            });
            let (w, status, okb, done) = (w.clone(), status.clone(), okb.clone(), done.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(150), move || match rx.try_recv() {
                Ok(Ok(_)) => {
                    w.close();
                    done();
                    gtk::glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    status.set_text(&e);
                    okb.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(_) => gtk::glib::ControlFlow::Break,
            });
        });
    }
    win.present();
    view.grab_focus();
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
    body.append(&crate::field("Access token (only if the MCP server needs one)", &token));
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
    advanced.append(&lbl("Sign-in. Most servers register Zohara Agent by themselves. If one does not, register Zohara Agent there with the redirect address http://127.0.0.1:PORT/callback (use the port below) and enter the client id.", "field-hint"));
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
                held: false,
                updated: 0,
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

/// The saved chats, inside the app: a list, and a click shows that conversation in the same panel (with a button to continue it).
pub fn saved_chats_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = crate::sheet(ui, "Saved chats", 720);
    let close = secondary("Close");
    foot.append(&close);
    {
        let w = win.clone();
        close.connect_clicked(move |_| w.close());
    }
    fn list(ui: &Rc<Ui>, win: &crate::Sheet, body: &gtk::Box) {
        while let Some(c) = body.first_child() {
            body.remove(&c);
        }
        let all = chats::load();
        if all.is_empty() {
            body.append(&lbl("No saved chats yet.", "pc-meta"));
            return;
        }
        for chat in all {
            let r = hbox(10);
            r.add_css_class("settings-row");
            let col = vbox(3);
            col.set_hexpand(true);
            let title = if chat.title.trim().is_empty() { "Untitled chat".to_string() } else { chat.title.clone() };
            col.append(&lbl(&title, "pc-name"));
            let msgs = chat.entries.iter().filter(|e| e.kind == "msg-user").count();
            col.append(&lbl(&format!("{msgs} messages"), "pc-meta"));
            r.append(&col);
            let view = secondary("Read");
            view.set_valign(gtk::Align::Center);
            r.append(&view);
            let (ui2, win2, body2) = (ui.clone(), win.clone(), body.clone());
            view.connect_clicked(move |_| preview(&ui2, &win2, &body2, chat.clone()));
            body.append(&r);
        }
    }
    fn preview(ui: &Rc<Ui>, win: &crate::Sheet, body: &gtk::Box, chat: chats::Chat) {
        while let Some(c) = body.first_child() {
            body.remove(&c);
        }
        let bar = hbox(8);
        let back = secondary("‹ All chats");
        let cont = crate::primary("Continue this chat");
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        bar.append(&back);
        bar.append(&spacer);
        bar.append(&cont);
        body.append(&bar);
        body.append(&lbl(&chat.title, "pc-name"));
        for e in &chat.entries {
            let who = if e.kind == "msg-user" { "You" } else if e.kind == "msg-error" { "Error" } else { "Zohara Agent" };
            let t = lbl(who, "pc-meta");
            t.set_xalign(0.0);
            body.append(&t);
            let m = lbl(&e.text, "pc-name");
            m.set_wrap(true);
            m.set_selectable(true);
            m.set_xalign(0.0);
            body.append(&m);
        }
        let (ui2, win2, body2) = (ui.clone(), win.clone(), body.clone());
        back.connect_clicked(move |_| list(&ui2, &win2, &body2));
        let (ui3, win3) = (ui.clone(), win.clone());
        cont.connect_clicked(move |_| {
            win3.close();
            crate::open_chat(&ui3, chat.clone());
        });
    }
    list(ui, &win, &body);
    win.present();
}

/// A small chat that only edits the memory: each message is an instruction, the answer says what changed.
fn memory_chat_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = crate::sheet(ui, "Chat about memory", 560);
    let log = vbox(8);
    body.append(&log);
    let hint = lbl("Say what to change. Zohara Agent edits the notes and tells you what it did.", "field-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    log.append(&hint);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("For example: join all my preferences into one note"));
    entry.add_css_class("field-input");
    entry.set_hexpand(true);
    let send = crate::primary("Send");
    let done = secondary("Done");
    foot.set_halign(gtk::Align::Fill);
    foot.append(&entry);
    foot.append(&send);
    foot.append(&done);
    {
        let (w, ui2) = (win.clone(), ui.clone());
        done.connect_clicked(move |_| {
            w.close();
            show(&ui2);
        });
    }
    let run: Rc<dyn Fn()> = {
        let (entry, log, send) = (entry.clone(), log.clone(), send.clone());
        Rc::new(move || {
            let text = entry.text().to_string();
            if text.trim().is_empty() || !send.is_sensitive() {
                return;
            }
            entry.set_text("");
            let you = lbl(&text, "msg-user");
            you.set_wrap(true);
            you.set_xalign(0.0);
            log.append(&you);
            let reply = lbl("Working on it…", "pc-meta");
            reply.set_wrap(true);
            reply.set_xalign(0.0);
            log.append(&reply);
            send.set_sensitive(false);
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(zo_core::ai::memory_agent::edit_by_instruction(&text));
            });
            let send = send.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(150), move || match rx.try_recv() {
                Ok(r) => {
                    reply.set_text(&r.unwrap_or_else(|e| e));
                    send.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(_) => gtk::glib::ControlFlow::Break,
            });
        })
    };
    {
        let r = run.clone();
        send.connect_clicked(move |_| r());
    }
    entry.connect_activate(move |_| run());
    win.present();
    entry.grab_focus();
}

/// "Add machine": where it is, who to log in as, and the private key (pasted; it goes to the keyring, not to the list).
fn add_machine_sheet(ui: &Rc<Ui>) {
    let (win, body, foot) = crate::sheet(ui, "Add a machine", 560);
    let entry = |hint: &str| {
        let e = gtk::Entry::new();
        e.set_placeholder_text(Some(hint));
        e.add_css_class("field-input");
        e
    };
    let (name, host, user, port, note) = (entry("cloud-vm"), entry("203.0.113.7  or  vm.example.com"), entry("ubuntu"), entry("22"), entry("what it is for (optional)"));
    body.append(&crate::field("Name (letters, digits, . _ -)", &name));
    body.append(&crate::field("Address", &host));
    body.append(&crate::field("User", &user));
    body.append(&crate::field("Port", &port));
    body.append(&crate::field("Note", &note));
    let key = gtk::TextView::new();
    key.set_monospace(true);
    key.set_wrap_mode(gtk::WrapMode::Char);
    key.set_top_margin(8);
    key.set_bottom_margin(8);
    key.set_left_margin(10);
    key.set_right_margin(10);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&key));
    scroll.set_min_content_height(120);
    scroll.add_css_class("field-input");
    let paste = secondary("Paste key");
    {
        let key = key.clone();
        paste.connect_clicked(move |b| {
            let key = key.clone();
            b.clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |t| {
                if let Ok(Some(t)) = t {
                    key.buffer().set_text(&t);
                }
            });
        });
    }
    let key_head = hbox(8);
    key_head.append(&lbl("Private key", "field-label"));
    paste.set_halign(gtk::Align::End);
    paste.set_hexpand(true);
    key_head.append(&paste);
    body.append(&key_head);
    body.append(&scroll);
    body.append(&lbl("The text of the private key file, from -----BEGIN to -----END. It needs no passphrase. It is stored in the system keyring and is never shown again.", "field-hint"));
    let status = lbl("", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    body.append(&status);
    let cancel = secondary("Cancel");
    let add = crate::primary("Add machine");
    foot.append(&cancel);
    foot.append(&add);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }
    {
        let (w, ui2, name) = (win.clone(), ui.clone(), name.clone());
        add.connect_clicked(move |_| {
            let buf = key.buffer();
            let key_text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            let result = zo_core::store::machines::parse_port(&port.text()).and_then(|p| {
                let m = zo_core::store::machines::Machine {
                    name: name.text().trim().into(),
                    host: host.text().trim().into(),
                    user: user.text().trim().into(),
                    port: p,
                    note: note.text().trim().into(),
                };
                zo_core::store::machines::add(m, Some(&key_text))
            });
            match result {
                Ok(()) => {
                    buf.set_text("");
                    w.close();
                    show(&ui2);
                }
                Err(e) => status.set_text(&e),
            }
        });
    }
    win.present();
    name.grab_focus();
}

#[cfg(test)]
mod account_tests {
    use super::*;

    #[test]
    fn home_card_rules() {
        assert!(card_visible(false, false));
        assert!(!card_visible(true, false));
        assert!(!card_visible(false, true));
        assert!(!card_visible(true, true));
    }

    #[test]
    fn verification_messages() {
        assert!(verification_text("a@b.c", true, true).starts_with("Account created. We sent a verification e-mail to a@b.c."));
        assert!(verification_text("a@b.c", true, false).contains("Resend"));
        assert!(verification_text("a@b.c", false, false).contains("not verified yet"));
    }

    #[test]
    fn browser_only_opens_safe_addresses() {
        assert!(browser_url_ok("https://zohara-agent.duckdns.org/device"));
        assert!(browser_url_ok("http://127.0.0.1:8080/device"));
        assert!(!browser_url_ok("http://evil.example/device"));
        assert!(!browser_url_ok("file:///etc/passwd"));
        assert!(!browser_url_ok("javascript:alert(1)"));
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// link:begin  Control this computer from the web, and Claude Code on the server (docs/LINK.md). Code: zo_core::link.

fn quote_sh(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn link_sections(ui: &Rc<Ui>, content: &gtk::Box) {
    use zo_core::link;
    let cfg = link::config();

    // ---- Control this computer from the web
    let (sec, card) = section("Control this computer from the web");
    let on = gtk::Switch::new();
    on.set_active(cfg.enabled);
    card.append(&row(
        "Let my web sessions run on this computer",
        "When this is on, Zohara Agent keeps one connection to the server, so the web app can offer \"My computer\" and use your tools, files and models here. The app has to be running (it can sit in the tray). When it is off, nothing is contacted for this.",
        Some(&on),
    ));
    let warn = lbl("Anyone who can sign in to your account on the web can run tools on this computer. Keep Ask-first on.", "pc-name");
    warn.set_wrap(true);
    warn.set_margin_start(16);
    warn.set_margin_end(16);
    warn.set_margin_bottom(8);
    card.append(&warn);
    let how = wrapped("Web sessions never get more than Auto mode, and anything that needs approval is asked here, on this computer, as a dialog.");
    how.set_margin_start(16);
    how.set_margin_end(16);
    how.set_margin_bottom(8);
    card.append(&how);
    let status = wrapped("");
    status.set_margin_start(16);
    status.set_margin_end(16);
    status.set_margin_bottom(12);
    card.append(&status);
    let line = || {
        let c = link::config();
        let s = link::state();
        if !c.enabled {
            "Off.".to_string()
        } else if s.active.is_some() {
            "A web session is running on this computer.".to_string()
        } else if s.connected {
            "On. This computer is reachable from the web.".to_string()
        } else if s.last_error.is_empty() {
            "On. Connecting to the server…".to_string()
        } else {
            format!("On, but not connected: {}", s.last_error)
        }
    };
    status.set_text(&line());
    {
        let status = status.clone();
        on.connect_state_set(move |_, state| {
            if let Err(e) = link::set_enabled(state) {
                status.set_text(&e);
            } else {
                status.set_text(&line());
            }
            gtk::glib::Propagation::Proceed
        });
    }
    {
        let status = status.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
            if status.root().is_none() {
                return gtk::glib::ControlFlow::Break;
            }
            status.set_text(&line());
            gtk::glib::ControlFlow::Continue
        });
    }
    let ex = gtk::Expander::new(Some("Advanced"));
    let inner = padded_box();
    let name = gtk::Entry::new();
    name.add_css_class("field-input");
    name.set_hexpand(true);
    name.set_placeholder_text(Some(&link::device_name()));
    name.set_text(&cfg.name);
    inner.append(&crate::field("Name shown in the web app", &name));
    let save = secondary("Save name");
    save.set_halign(gtk::Align::Start);
    inner.append(&save);
    let nst = wrapped("");
    inner.append(&nst);
    {
        let nst = nst.clone();
        save.connect_clicked(move |_| {
            nst.set_text(&match link::update_config(|c| c.name = name.text().to_string()) {
                Ok(()) => "Saved.".to_string(),
                Err(e) => e,
            });
        });
    }
    let appr = gtk::Switch::new();
    appr.set_active(cfg.approvals_from_web);
    inner.append(&row("Answer approvals from the web too", "Off (recommended): a question \"Allow?\" appears only on this computer. On: the web page can answer it, so anyone signed in to your account can approve.", Some(&appr)));
    appr.connect_state_set(|_, state| {
        let _ = link::update_config(|c| c.approvals_from_web = state);
        gtk::glib::Propagation::Proceed
    });
    let byp = gtk::Switch::new();
    byp.set_active(cfg.allow_bypass);
    inner.append(&row("Allow Bypass mode from the web", "Off (recommended): a web session that asks for Bypass runs in Auto. On: it may run without asking.", Some(&byp)));
    byp.connect_state_set(|_, state| {
        let _ = link::update_config(|c| c.allow_bypass = state);
        gtk::glib::Propagation::Proceed
    });
    ex.set_child(Some(&inner));
    ex.set_margin_top(6);
    ex.set_margin_bottom(10);
    ex.set_margin_start(16);
    ex.set_margin_end(16);
    card.append(&ex);
    content.append(&sec);

    // ---- Claude subscription on the web (owner only)
    let (sec, card) = section("Use my Claude subscription on the web (owner only)");
    card.append(&row(
        "",
        "Claude Code can run on the Zohara Agent server with your own Claude subscription, so you can use it from the web even when this computer is off. What is sent: one long-lived sign-in token made by \"claude setup-token\" (valid about a year). The server keeps it encrypted, only your account can use it, and you can remove it here at any time. Only the owner account of the server can do this; everybody else uses Claude Code on their own computer through the switch above.",
        no_control(),
    ));
    let secret: Rc<std::cell::RefCell<Option<link::Secret>>> = Rc::new(std::cell::RefCell::new(None));
    let cstatus = wrapped("");
    cstatus.set_margin_start(16);
    cstatus.set_margin_end(16);
    cstatus.set_margin_bottom(10);
    let sent = |at: u64| {
        if at == 0 {
            "Nothing sent yet.".to_string()
        } else {
            format!("Sent {} ago. The token lasts about a year; send a new one when Claude stops accepting it.", {
                let ms = zo_core::activity::now_ms().saturating_sub(at);
                if ms >= 48 * 3_600_000 { format!("{} days", ms / 86_400_000) } else { crate::tray::elapsed(ms) }
            })
        }
    };
    cstatus.set_text(&sent(cfg.claude_sent_at));
    let get = secondary("Get sign-in");
    let send = secondary("Send to my server");
    send.set_sensitive(false);
    let remove = secondary("Remove from server");
    let paste = gtk::PasswordEntry::new();
    paste.set_show_peek_icon(false);
    paste.set_hexpand(true);
    paste.add_css_class("field-input");
    let usepaste = secondary("Use pasted token");
    let bar = hbox(8);
    bar.set_margin_start(16);
    bar.set_margin_end(16);
    bar.set_margin_bottom(8);
    bar.append(&get);
    bar.append(&send);
    bar.append(&remove);
    card.append(&bar);
    let pbar = hbox(8);
    pbar.set_margin_start(16);
    pbar.set_margin_end(16);
    pbar.set_margin_bottom(8);
    pbar.append(&paste);
    pbar.append(&usepaste);
    let pbox = vbox(4);
    pbox.set_margin_start(0);
    pbox.append(&lbl("   Or paste the token yourself (it is hidden and never shown again):", "field-hint"));
    pbox.append(&pbar);
    card.append(&pbox);
    card.append(&cstatus);

    {
        let (cstatus, secret, send, ui) = (cstatus.clone(), secret.clone(), send.clone(), ui.clone());
        get.connect_clicked(move |b| {
            let Some(bin) = claude_code::binary() else {
                cstatus.set_text("Claude Code is not installed on this computer. Install it, then try again (or paste a token below).");
                return;
            };
            let term = match zo_core::term::open("Claude sign-in", None, Some(&format!("{} setup-token", quote_sh(&bin.display().to_string()))), false) {
                Ok(t) => t,
                Err(e) => {
                    cstatus.set_text(&e);
                    return;
                }
            };
            b.set_sensitive(false);
            ui.dock.show("terminal");
            cstatus.set_text("Follow the steps in the Terminal panel. The token is picked up here automatically; it is not shown.");
            let (cstatus, secret, send, b) = (cstatus.clone(), secret.clone(), send.clone(), b.clone());
            let started = std::time::Instant::now();
            let ended_once = std::cell::Cell::new(false);
            gtk::glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
                let leave = |msg: &str| {
                    term.close();
                    cstatus.set_text(msg);
                    b.set_sensitive(true);
                    gtk::glib::ControlFlow::Break
                };
                if cstatus.root().is_none() {
                    term.close();
                    return gtk::glib::ControlFlow::Break;
                }
                if let Some(tok) = link::extract_token(&term.screen_text()) {
                    *secret.borrow_mut() = Some(tok);
                    send.set_sensitive(true);
                    return leave("Sign-in captured (hidden). Press \"Send to my server\".");
                }
                if !term.alive() {
                    // one more look after it ended: the last text may still be arriving
                    if ended_once.replace(true) {
                        return leave("The sign-in ended without a token. Try again, or paste the token below.");
                    }
                } else if started.elapsed() > std::time::Duration::from_secs(600) {
                    return leave("That took too long. Try again.");
                }
                gtk::glib::ControlFlow::Continue
            });
        });
    }
    {
        let (cstatus, secret, send, paste) = (cstatus.clone(), secret.clone(), send.clone(), paste.clone());
        usepaste.connect_clicked(move |_| {
            let text = paste.text().to_string();
            paste.set_text("");
            match link::extract_token(&text) {
                Some(t) => {
                    *secret.borrow_mut() = Some(t);
                    send.set_sensitive(true);
                    cstatus.set_text("Token taken (hidden). Press \"Send to my server\".");
                }
                None => cstatus.set_text("That does not look like a Claude token (it starts with sk-ant-oat)."),
            }
        });
    }
    {
        let (cstatus, secret) = (cstatus.clone(), secret.clone());
        send.connect_clicked(move |b| {
            let Some(tok) = secret.borrow_mut().take() else { return };
            b.set_sensitive(false);
            cstatus.set_text("Sending…");
            let (b, cstatus, secret) = (b.clone(), cstatus.clone(), secret.clone());
            background(
                move || {
                    let r = link::send_claude_token(&tok);
                    (r, tok)
                },
                move |(r, tok)| match r {
                    Ok(()) => {
                        // sent: the token is wiped (dropped) here
                        drop(tok);
                        cstatus.set_text("Sent. Claude Code (your subscription) now shows in the model list on the web.");
                    }
                    Err(e) => {
                        *secret.borrow_mut() = Some(tok);
                        b.set_sensitive(true);
                        cstatus.set_text(&e);
                    }
                },
            );
        });
    }
    {
        let (cstatus, secret, send) = (cstatus.clone(), secret.clone(), send.clone());
        remove.connect_clicked(move |b| {
            b.set_sensitive(false);
            *secret.borrow_mut() = None;
            send.set_sensitive(false);
            cstatus.set_text("Removing…");
            let (b, cstatus) = (b.clone(), cstatus.clone());
            background(link::remove_claude_token, move |r| {
                b.set_sensitive(true);
                cstatus.set_text(&match r {
                    Ok(()) => "Removed from the server.".to_string(),
                    Err(e) => e,
                });
            });
        });
    }
    content.append(&sec);
}
// link:end
