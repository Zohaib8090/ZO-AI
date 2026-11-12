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
        ("personality", "Personality", "How ZO-AI talks to you."),
        ("memory", "Memory", "What ZO-AI remembers about you between chats."),
        ("general", "Appearance and permissions", "Light or dark, and what ZO-AI may do on its own."),
        ("apps", "Connected apps", "Other programs that use ZO-AI, and tools ZO-AI can use."),
        ("sync", "Web sync", "Keep chats, memory and personalities the same on your ZO-AI server and the web app."),
        ("machines", "Machines", "Other computers ZO-AI can work on over SSH."),
        ("data", "Data", "Saved chats."),
        ("about", "About", "Version."),
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
        "Manual asks before every change and command. Auto lets looking around run and asks before changes. Bypass never asks. The same rule applies to every app connected to ZO-AI.",
        Some(&dd),
    ));
    content.append(&sec);
    }

    // ---- other apps ----
    if page == "apps" {
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
    }

    // ---- machines: other computers over SSH ----
    if page == "machines" {
        let (sec, card) = section("Machines");
        let add = secondary("Add machine");
        let ui2 = ui.clone();
        add.connect_clicked(move |_| add_machine_sheet(&ui2));
        card.append(&row(
            "Let ZO-AI work on another computer",
            "A cloud VM or a server you can reach with SSH. ZO-AI can run commands there and read and write files, asking you first unless the mode is Bypass. The private key is kept in the system keyring.",
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
                crate::confirm_delete(&ui2, &format!("Remove \"{name}\"?"), "ZO-AI forgets this machine and its private key. Nothing changes on the machine itself.", move || {
                    let _ = zo_core::store::machines::remove(&name3);
                    show(&ui3);
                });
            });
            r.append(&del);
            card.append(&r);
        }
        content.append(&sec);
    }

    // ---- personality: how ZO-AI talks ----
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
        card.append(&row("Plain", "ZO-AI's own manner: clear and to the point.", Some(&plain)));
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
        card.append(&row("Add a personality", "The AI can write one from a description (it uses the model you chose). A personality changes the voice only, never the rules or what ZO-AI may do.", Some(&both)));
        content.append(&sec);
    }

    // ---- memory: what ZO-AI remembers about you ----
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
            &format!("{notes} notes about you and {chats_n} chat summaries, shared with the phone. After every {} messages of yours, ZO-AI reads the chat in the background and keeps what is worth remembering. Never passwords or keys.", st.every),
            Some(&sw),
        ));
        let talk = crate::primary("Chat about memory…");
        {
            let ui2 = ui.clone();
            talk.connect_clicked(move |_| memory_chat_sheet(&ui2));
        }
        card.append(&row("Edit by talking", "Tell ZO-AI what to change, in your own words: \"join my preferences into one note\", \"forget the old laptop\", \"my name is spelled Zohaib\". This chat only edits the notes.", Some(&talk)));
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
                        text_sheet(&ui2, "Edit note", &text, "What should ZO-AI remember?", "I prefer short answers", "Save", false, move |t| {
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
                text_sheet(&ui2, "Add a note", "", "What should ZO-AI remember?", "I prefer short answers", "Add", false, |t| {
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
        card.append(&row("Your notes", "Pin a note to make sure the AI always knows it. Delete one that is wrong. You can also just tell ZO-AI: \"remember that ...\" or \"forget that ...\".", Some(&both)));
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

    // ---- web sync ----
    if page == "sync" {
    use zo_core::sync;
    let (sec, card) = section("Server");
    let cfg = sync::config();
    let url = gtk::Entry::new();
    url.add_css_class("field-input");
    url.set_placeholder_text(Some("https://..."));
    url.set_text(&cfg.url);
    url.set_hexpand(true);
    let token = gtk::PasswordEntry::new();
    token.set_show_peek_icon(true);
    token.add_css_class("field-input");
    token.set_hexpand(true);
    token.set_placeholder_text(Some(if sync::token().is_some() { "Saved in the keyring. Type to replace it" } else { "Paste the device token (starts with zd_)" }));
    let on = gtk::Switch::new();
    on.set_active(cfg.enabled);
    let status = lbl("", "field-hint");
    status.set_wrap(true);
    status.set_xalign(0.0);
    let fields = vbox(12);
    fields.set_margin_top(4);
    fields.set_margin_bottom(14);
    fields.set_margin_start(16);
    fields.set_margin_end(16);
    fields.append(&crate::field("Server address", &url));
    fields.append(&crate::field("Device token", &token));
    fields.append(&lbl("Get one in the web app: sign in with Google, then Settings > Account > Create device token > Sync a desktop app. It is shown once; it is kept in the system keyring here and is never shown again. You can revoke it any time on the same page.", "field-hint"));
    let save = crate::primary("Save");
    let now = secondary("Sync now");
    let buttons = hbox(8);
    buttons.append(&save);
    buttons.append(&now);
    fields.append(&buttons);
    fields.append(&status);
    card.append(&fields);
    // saves the address, the token (when one was typed) and the switch; says what is wrong in the status line
    let store: Rc<dyn Fn() -> Result<(), String>> = {
        let (url, token, on) = (url.clone(), token.clone(), on.clone());
        Rc::new(move || {
            let typed = token.text().to_string();
            if !typed.trim().is_empty() {
                sync::set_token(&sync::check_device_token(&typed)?)?;
                token.set_text("");
                token.set_placeholder_text(Some("Saved in the keyring. Type to replace it"));
            }
            sync::save_config(&url.text(), on.is_active())
        })
    };
    {
        let (store, status) = (store.clone(), status.clone());
        save.connect_clicked(move |_| match store() {
            Ok(()) => status.set_text("Saved."),
            Err(e) => status.set_text(&e),
        });
    }
    {
        let (store, status) = (store.clone(), status.clone());
        now.connect_clicked(move |b| {
            if let Err(e) = store() {
                status.set_text(&e);
                return;
            }
            b.set_sensitive(false);
            status.set_text("Syncing…");
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(sync::sync_now());
            });
            let (b, status) = (b.clone(), status.clone());
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(150), move || match rx.try_recv() {
                Ok(r) => {
                    b.set_sensitive(true);
                    status.set_text(&sync_line());
                    let _ = r;
                    gtk::glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(_) => gtk::glib::ControlFlow::Break,
            });
        });
    }
    {
        let (store, status) = (store.clone(), status.clone());
        on.connect_state_set(move |_, _| {
            if let Err(e) = store() {
                status.set_text(&e);
            }
            gtk::glib::Propagation::Proceed
        });
    }
    card.append(&row("Keep in step", "Every 30 seconds this computer and the server compare chats, memory notes and personalities. The newest change wins; a deleted chat or note is deleted on both. API keys are never sent.", Some(&on)));
    status.set_text(&sync_line());
    // the status line follows the background sync while this page is open
    {
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
    }

    // ---- about ----
    if page == "about" {
    let (sec, card) = section("About");
    card.append(&row("ZO-AI", &format!("Version {} · the AI built into Zohara OS", env!("CARGO_PKG_VERSION")), no_control()));
    let _ = agent::SYSTEM_PROMPT;
    content.append(&sec);
    }

    ui.stack.set_visible_child_name("settings");
    set_nav(ui, "settings");
    ui.top_title.set_text("Settings");
}

/// The sync status as one line for the Web sync page.
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
            let who = if e.kind == "msg-user" { "You" } else if e.kind == "msg-error" { "Error" } else { "ZO-AI" };
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
    let hint = lbl("Say what to change. ZO-AI edits the notes and tells you what it did.", "field-hint");
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
