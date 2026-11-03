//! ZO-AI — the OS-level AI module for Zohara OS.
//!
//! Rust port of the approved HTML design: sidebar, chat, project list,
//! project workspace with instructions / context / memory / schedules.

mod ask;
mod chat;
mod chats;
mod composer;
mod markdown;
mod model;
mod quick;
mod shortcut;

use adw::prelude::*;
use gtk4 as gtk;
use gtk::{gio, glib, Orientation};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use model::{Attachment, Project, Schedule, Store};

const APP_ID: &str = "org.zohara.ZoAi";
const CSS: &str = include_str!("style.css");

// ---------------------------------------------------------------- widgets

fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(Orientation::Horizontal, spacing)
}

fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(Orientation::Vertical, spacing)
}

fn lbl(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    if !class.is_empty() {
        l.add_css_class(class);
    }
    l
}

/// The ZO-AI mark without its tile, built into the program so it shows even before the icon is installed.
fn mark(size: i32) -> gtk::Image {
    let bytes = glib::Bytes::from_static(include_bytes!("../data/zo-ai-mark.png"));
    let img = match gtk::gdk::Texture::from_bytes(&bytes) {
        Ok(t) => gtk::Image::from_paintable(Some(&t)),
        Err(_) => gtk::Image::new(),
    };
    img.set_pixel_size(size);
    img.set_halign(gtk::Align::Center);
    img.set_valign(gtk::Align::Center);
    img
}

fn icon(name: &str, size: i32) -> gtk::Image {
    let img = gtk::Image::from_icon_name(name);
    img.set_pixel_size(size);
    img
}

fn icon_button(name: &str, tip: &str, class: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(name);
    b.set_has_frame(false);
    b.set_tooltip_text(Some(tip));
    for c in class.split_whitespace() {
        b.add_css_class(c);
    }
    b
}

fn nav_button(icon_name: &str, text: &str, active: bool, badge: Option<&str>) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_has_frame(false);
    b.add_css_class("nav-item");
    if active {
        b.add_css_class("active");
    }

    let row = hbox(12);
    row.append(&icon(icon_name, 18));
    let l = lbl(text, "");
    l.set_hexpand(true);
    row.append(&l);
    if let Some(n) = badge {
        row.append(&lbl(n, "badge"));
    }
    b.set_child(Some(&row));
    b
}

fn rail_button(icon_name: &str, text: &str) -> (gtk::Button, gtk::Label) {
    let b = gtk::Button::new();
    b.set_has_frame(false);
    b.add_css_class("rail-row");

    let row = hbox(11);
    row.append(&icon(icon_name, 16));
    let name = lbl(text, "rail-label");
    name.set_hexpand(true);
    row.append(&name);
    let action = lbl("Add", "rail-action");
    row.append(&action);

    b.set_child(Some(&row));
    (b, action)
}

fn pop_row(
    icon_name: Option<&str>,
    title: &str,
    sub: Option<&str>,
    trail: Option<&str>,
    accent: bool,
) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_has_frame(false);
    b.add_css_class("pop-row");

    let row = hbox(10);
    if let Some(name) = icon_name {
        row.append(&icon(name, 16));
    }

    let col = vbox(2);
    col.set_hexpand(true);
    col.append(&lbl(title, "pop-title"));
    if let Some(s) = sub {
        col.append(&lbl(s, "pop-sub"));
    }
    row.append(&col);

    if let Some(t) = trail {
        let tag = lbl(t, "pop-trail");
        if accent {
            tag.add_css_class("accent");
        }
        row.append(&tag);
    }

    b.set_child(Some(&row));
    b
}

// ---------------------------------------------------------------- state

struct Ui {
    window: adw::ApplicationWindow,
    stack: gtk::Stack,
    reveal: gtk::Revealer,
    pinned: Cell<bool>,

    store: RefCell<Store>,
    current: RefCell<Option<String>>,
    sort_mode: Cell<u8>,
    query: RefCell<String>,

    nav_new: gtk::Button,
    nav_search: gtk::Button,
    nav_projects: gtk::Button,
    nav_artifacts: gtk::Button,
    nav_scheduled: gtk::Button,

    proj_list: gtk::Box,
    search_entry: gtk::Entry,

    pv_title: gtk::Label,
    pv_prompt: composer::Prompt,
    pv_hint: gtk::Label,

    model_label: gtk::Label,
    perm_label: gtk::Label,
    pv_model_label: gtk::Label,
    pv_perm_label: gtk::Label,
    top_title: gtk::Label,
    pinned_label: gtk::Label,
    pinned_box: gtk::Box,
    recents_label: gtk::Label,
    recents_box: gtk::Box,
    side_settings: gtk::Button,
    chat: Rc<chat::Ctl>,
    search_input: gtk::Entry,
    search_results: gtk::Box,

    rail_instructions: gtk::Label,
    rail_context: gtk::Label,
    rail_memory: gtk::Label,
    rail_scheduled: gtk::Label,
}

fn save_store(ui: &Rc<Ui>) {
    ui.store.borrow().save();
    refresh_sidebar(ui);
}

/// The pinned projects in the sidebar.
fn refresh_sidebar(ui: &Rc<Ui>) {
    while let Some(c) = ui.pinned_box.first_child() {
        ui.pinned_box.remove(&c);
    }
    let store = ui.store.borrow();
    let mut pinned: Vec<&Project> = store.projects.iter().filter(|p| p.pinned).collect();
    pinned.sort_by(|a, b| b.updated.cmp(&a.updated));
    ui.pinned_label.set_visible(!pinned.is_empty());
    for p in pinned {
        let b = gtk::Button::new();
        b.set_has_frame(false);
        b.add_css_class("side-item");
        let row = hbox(10);
        row.append(&icon("folder-symbolic", 14));
        let name = lbl(&p.name, "");
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_hexpand(true);
        row.append(&name);
        b.set_child(Some(&row));
        let (ui2, id) = (ui.clone(), p.id.clone());
        b.connect_clicked(move |_| open_project(&ui2, &id));
        ui.pinned_box.append(&b);
    }
}

/// The search page: chats and projects that match what is typed (everything recent when it is empty).
fn refresh_search(ui: &Rc<Ui>) {
    while let Some(c) = ui.search_results.first_child() {
        ui.search_results.remove(&c);
    }
    let query = ui.search_input.text().to_string();
    let q = query.trim().to_lowercase();
    let row = |icon_name: &str, title: &str, sub: &str, when: u64| {
        let b = gtk::Button::new();
        b.set_has_frame(false);
        b.add_css_class("project-card");
        let inner = hbox(14);
        let ic = icon(icon_name, 18);
        ic.add_css_class("pc-icon");
        inner.append(&ic);
        let col = vbox(3);
        col.set_hexpand(true);
        let t = lbl(title, "pc-name");
        t.set_ellipsize(gtk::pango::EllipsizeMode::End);
        col.append(&t);
        if !sub.is_empty() {
            let s = lbl(sub, "pc-meta");
            s.set_ellipsize(gtk::pango::EllipsizeMode::End);
            col.append(&s);
        }
        inner.append(&col);
        inner.append(&lbl(&relative_of(when), "pc-meta"));
        b.set_child(Some(&inner));
        b
    };
    let mut shown = 0;
    // projects
    let projects: Vec<Project> = ui.store.borrow().projects.iter().filter(|p| !q.is_empty() && (p.name.to_lowercase().contains(&q) || p.goal.to_lowercase().contains(&q))).cloned().collect();
    for p in projects {
        let b = row("folder-symbolic", &p.name, &p.goal, p.updated);
        let (ui2, id) = (ui.clone(), p.id.clone());
        b.connect_clicked(move |_| open_project(&ui2, &id));
        ui.search_results.append(&b);
        shown += 1;
    }
    // chats
    let all = chats::load();
    for hit in chats::search(&query, &all).into_iter().take(50) {
        let b = row("chat-message-new-symbolic", &hit.title, &hit.snippet, hit.updated);
        let (ui2, chat) = (ui.clone(), all.iter().find(|c| c.id == hit.id).cloned());
        b.connect_clicked(move |_| {
            if let Some(chat) = chat.clone() {
                ui2.chat.load_chat(chat.clone());
                ui2.stack.set_visible_child_name("chat");
                set_nav(&ui2, "new");
                ui2.top_title.set_text(&chat.title);
            }
        });
        ui.search_results.append(&b);
        shown += 1;
    }
    if shown == 0 {
        let none = lbl(if q.is_empty() { "No chats yet. Start one and it will show up here." } else { "Nothing matches that." }, "empty-text");
        none.set_margin_top(24);
        ui.search_results.append(&none);
    }
}

/// The recent chats in the sidebar.
fn refresh_recents(ui: &Rc<Ui>) {
    while let Some(c) = ui.recents_box.first_child() {
        ui.recents_box.remove(&c);
    }
    let all = chats::load();
    ui.recents_label.set_visible(!all.is_empty());
    for chat in all.into_iter().take(15) {
        let row = hbox(0);
        let b = gtk::Button::new();
        b.set_has_frame(false);
        b.set_hexpand(true);
        b.add_css_class("side-item");
        let inner = hbox(10);
        inner.append(&icon("chat-message-new-symbolic", 14));
        let name = lbl(&chat.title, "");
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_hexpand(true);
        inner.append(&name);
        b.set_child(Some(&inner));
        let del = icon_button("edit-delete-symbolic", "Delete this chat", "icon-btn side-del");
        row.append(&b);
        row.append(&del);
        let (ui2, id, shown) = (ui.clone(), chat.id.clone(), chat.clone());
        b.connect_clicked(move |_| {
            ui2.chat.load_chat(shown.clone());
            ui2.stack.set_visible_child_name("chat");
            set_nav(&ui2, "new");
            ui2.top_title.set_text(&shown.title);
            let _ = &id;
        });
        let (ui2, id) = (ui.clone(), chat.id.clone());
        del.connect_clicked(move |_| {
            chats::delete(&id);
            refresh_recents(&ui2);
        });
        ui.recents_box.append(&row);
    }
}

/// What the model should know about the open project.
fn project_context(ui: &Rc<Ui>) -> String {
    let current = ui.current.borrow().clone();
    let store = ui.store.borrow();
    let Some(p) = current.and_then(|id| store.get(&id).cloned()) else { return String::new() };
    let mut s = format!("You are working inside the project \"{}\".", p.name);
    if !p.goal.trim().is_empty() {
        s += &format!("\nGoal: {}", p.goal.trim());
    }
    if !p.instructions.trim().is_empty() {
        s += &format!("\nInstructions for this project:\n{}", p.instructions.trim());
    }
    if !p.memory.is_empty() {
        s += "\nWhat you remember about this project:";
        for m in &p.memory {
            s += &format!("\n- {}", m.text);
        }
    }
    s
}

fn set_nav(ui: &Rc<Ui>, which: &str) {
    ui.top_title.set_text(match which {
        "projects" => "Projects",
        "artifacts" => "Artifacts",
        "scheduled" => "Scheduled",
        _ => "",
    });
    for (btn, key) in [
        (&ui.nav_new, "new"),
        (&ui.nav_search, "search"),
        (&ui.nav_projects, "projects"),
        (&ui.nav_artifacts, "artifacts"),
        (&ui.nav_scheduled, "scheduled"),
    ] {
        if key == which {
            btn.add_css_class("active");
        } else {
            btn.remove_css_class("active");
        }
    }
}

// ---------------------------------------------------------------- projects page

fn refresh_projects(ui: &Rc<Ui>) {
    while let Some(child) = ui.proj_list.first_child() {
        ui.proj_list.remove(&child);
    }

    let store = ui.store.borrow();
    let mut list = store.projects.clone();

    let query = ui.query.borrow().trim().to_lowercase();
    if !query.is_empty() {
        list.retain(|p| p.name.to_lowercase().contains(&query));
    }

    match ui.sort_mode.get() {
        1 => list.sort_by(|a, b| b.created.cmp(&a.created)),
        2 => list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        _ => list.sort_by(|a, b| b.updated.cmp(&a.updated)),
    }
    list.sort_by(|a, b| b.pinned.cmp(&a.pinned));

    if list.is_empty() {
        let empty = vbox(10);
        empty.add_css_class("empty-box");
        empty.set_valign(gtk::Align::Center);
        empty.set_halign(gtk::Align::Center);

        let t = lbl("No projects yet", "empty-title");
        t.set_halign(gtk::Align::Center);
        let d = lbl(
            "Projects keep one long-running thing together — its files, notes and context — so ZO-AI keeps working inside it.",
            "empty-text",
        );
        d.set_halign(gtk::Align::Center);
        d.set_justify(gtk::Justification::Center);
        d.set_wrap(true);
        d.set_max_width_chars(56);

        let b = gtk::Button::with_label("New project");
        b.add_css_class("page-btn");
        b.add_css_class("primary");
        b.set_halign(gtk::Align::Center);
        b.set_margin_top(10);
        {
            let ui2 = ui.clone();
            b.connect_clicked(move |_| open_new_project(&ui2));
        }

        empty.append(&t);
        empty.append(&d);
        empty.append(&b);

        let wrap = vbox(0);
        wrap.set_valign(gtk::Align::Center);
        wrap.set_vexpand(true);
        wrap.append(&empty);
        ui.proj_list.append(&wrap);
        return;
    }

    let flow = gtk::FlowBox::new();
    flow.set_max_children_per_line(2);
    flow.set_min_children_per_line(1);
    flow.set_homogeneous(true);
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);

    for p in list {
        flow.insert(&project_card(ui, &p), -1);
    }
    ui.proj_list.append(&flow);
}

fn project_card(ui: &Rc<Ui>, p: &Project) -> gtk::Widget {
    let card = hbox(2);
    card.add_css_class("project-card");

    let hit = gtk::Button::new();
    hit.set_has_frame(false);
    hit.set_hexpand(true);

    let inner = hbox(14);
    let ic = icon("folder-symbolic", 18);
    ic.add_css_class("pc-icon");
    inner.append(&ic);

    let col = vbox(3);
    col.set_hexpand(true);
    col.append(&lbl(&p.name, "pc-name"));
    col.append(&lbl(&relative_of(p.updated), "pc-meta"));
    inner.append(&col);

    hit.set_child(Some(&inner));
    card.append(&hit);

    let pin = icon_button("view-pin-symbolic", "Pin", "pc-pin");
    if p.pinned {
        pin.add_css_class("pinned");
    }
    card.append(&pin);

    {
        let ui2 = ui.clone();
        let id = p.id.clone();
        hit.connect_clicked(move |_| open_project(&ui2, &id));
    }

    {
        let ui2 = ui.clone();
        let id = p.id.clone();
        pin.connect_clicked(move |_| {
            {
                let mut store = ui2.store.borrow_mut();
                if let Some(project) = store.get_mut(&id) {
                    project.pinned = !project.pinned;
                }
            }
            save_store(&ui2);
            refresh_projects(&ui2);
        });
    }

    card.upcast()
}

fn relative_of(ts: u64) -> String {
    model::relative(ts)
}

// ---------------------------------------------------------------- project workspace

fn open_project(ui: &Rc<Ui>, id: &str) {
    let project = match ui.store.borrow().get(id) {
        Some(p) => p.clone(),
        None => return,
    };

    ui.pv_title.set_text(&project.name);
    ui.pv_prompt.set_placeholder(&format!("New session in {}", project.name));
    ui.pv_hint.set_text(if project.goal.trim().is_empty() {
        "ZO-AI remembers the same knowledge every time you talk to it in this project."
    } else {
        &project.goal
    });

    *ui.current.borrow_mut() = Some(project.id.clone());
    update_rail(ui);
    ui.stack.set_visible_child_name("project");
    set_nav(ui, "projects");
    ui.top_title.set_text(&format!("Projects  /  {}", project.name));
}

fn update_rail(ui: &Rc<Ui>) {
    let current = ui.current.borrow().clone();
    let Some(id) = current else { return };
    let store = ui.store.borrow();
    let Some(p) = store.get(&id) else { return };

    ui.rail_instructions.set_text(if p.instructions.trim().is_empty() {
        "Add"
    } else {
        "Edit"
    });

    let ctx = p.context.len();
    ui.rail_context.set_text(&if ctx > 0 {
        format!("Add · {}", ctx)
    } else {
        "Add".to_string()
    });

    let mem = p.memory.len();
    ui.rail_memory.set_text(&if mem > 0 {
        format!("View · {}", mem)
    } else {
        "View".to_string()
    });

    let sch = p.schedules.len();
    ui.rail_scheduled.set_text(&if sch > 0 {
        format!("Add · {}", sch)
    } else {
        "Add".to_string()
    });
}

// ---------------------------------------------------------------- sheets

fn sheet(ui: &Rc<Ui>, title: &str, width: i32) -> (gtk::Window, gtk::Box, gtk::Box) {
    let win = gtk::Window::builder()
        .title(title)
        .transient_for(&ui.window)
        .modal(true)
        .default_width(width)
        .build();
    win.add_css_class("sheet");

    let root = vbox(0);
    root.add_css_class("zo-root");

    let head = hbox(8);
    head.add_css_class("sheet-head");
    let t = lbl(title, "sheet-title");
    t.set_hexpand(true);
    head.append(&t);

    let close = icon_button("window-close-symbolic", "Close", "sheet-close");
    head.append(&close);

    let body = vbox(12);
    body.add_css_class("sheet-body");
    body.set_vexpand(true);

    let foot = hbox(8);
    foot.add_css_class("sheet-foot");
    foot.set_halign(gtk::Align::End);
    foot.set_valign(gtk::Align::End);

    root.append(&head);
    root.append(&body);
    root.append(&foot);
    win.set_child(Some(&root));

    {
        let w = win.clone();
        close.connect_clicked(move |_| w.close());
    }

    (win, body, foot)
}

fn field(label: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = vbox(7);
    b.append(&lbl(label, "field-label"));
    b.append(widget);
    b
}

fn primary(text: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("btn");
    b.add_css_class("primary");
    b
}

fn secondary(text: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("btn");
    b
}

fn open_instructions(ui: &Rc<Ui>) {
    let Some(id) = ui.current.borrow().clone() else {
        return;
    };
    let existing = ui
        .store
        .borrow()
        .get(&id)
        .map(|p| p.instructions.clone())
        .unwrap_or_default();

    let (win, body, foot) = sheet(ui, "Instructions", 620);

    let hint = lbl(
        "Sits one layer above the system prompt, for this project only.",
        "field-hint",
    );
    hint.set_wrap(true);

    let view = gtk::TextView::new();
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.buffer().set_text(&existing);
    view.add_css_class("field-input");

    let scroll = gtk::ScrolledWindow::builder()
        .child(&view)
        .min_content_height(150)
        .build();

    body.append(&field("How should ZO-AI work in this project?", &scroll));
    body.append(&hint);

    let cancel = secondary("Cancel");
    let save = primary("Save");
    foot.append(&cancel);
    foot.append(&save);

    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }

    {
        let ui2 = ui.clone();
        let w = win.clone();
        save.connect_clicked(move |_| {
            let buffer = view.buffer();
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            {
                let mut store = ui2.store.borrow_mut();
                if let Some(p) = store.get_mut(&id) {
                    p.instructions = text.to_string();
                    p.updated = model::now_millis();
                }
            }
            save_store(&ui2);
            update_rail(&ui2);
            w.close();
        });
    }

    win.present();
}

fn open_context(ui: &Rc<Ui>) {
    let Some(id) = ui.current.borrow().clone() else {
        return;
    };
    let (win, body, foot) = sheet(ui, "Context", 620);

    let list = vbox(8);
    body.append(&list);

    let add = gtk::Button::new();
    add.add_css_class("btn");
    add.set_child(Some(&{
        let row = hbox(7);
        row.append(&icon("list-add-symbolic", 14));
        row.append(&lbl("Add files", ""));
        row
    }));
    body.append(&add);

    let hint = lbl(
        "Documents, zips, code, images. Everything in context is available to ZO-AI in every session of this project.",
        "field-hint",
    );
    hint.set_wrap(true);
    body.append(&hint);

    let done = primary("Done");
    foot.append(&done);
    {
        let w = win.clone();
        done.connect_clicked(move |_| w.close());
    }

    fill_context_list(ui, &list, &id);

    let dialog = gtk::FileDialog::builder().title("Add files").build();
    let ui2 = ui.clone();
    let list2 = list.clone();
    let id2 = id.clone();
    add.connect_clicked(move |_| {
        let ui3 = ui2.clone();
        let list3 = list2.clone();
        let id3 = id2.clone();
        dialog.open_multiple(
            Some(&ui2.window),
            gio::Cancellable::NONE,
            move |res| {
                if let Ok(files) = res {
                    let mut added = Vec::new();
                    for i in 0..files.n_items() {
                        if let Some(obj) = files.item(i) {
                            if let Ok(file) = obj.downcast::<gio::File>() {
                                let name = file
                                    .basename()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "file".to_string());
                                let size = file
                                    .query_info(
                                        "standard::size",
                                        gio::FileQueryInfoFlags::NONE,
                                        gio::Cancellable::NONE,
                                    )
                                    .map(|info| info.size() as u64)
                                    .unwrap_or(0);
                                added.push(Attachment { name, size });
                            }
                        }
                    }
                    if !added.is_empty() {
                        {
                            let mut store = ui3.store.borrow_mut();
                            if let Some(p) = store.get_mut(&id3) {
                                p.context.extend(added);
                                p.updated = model::now_millis();
                            }
                        }
                        save_store(&ui3);
                        update_rail(&ui3);
                    }
                    fill_context_list(&ui3, &list3, &id3);
                }
            },
        );
    });

    win.present();
}

fn fill_context_list(ui: &Rc<Ui>, list: &gtk::Box, id: &str) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let items: Vec<Attachment> = ui
        .store
        .borrow()
        .get(id)
        .map(|p| p.context.clone())
        .unwrap_or_default();

    if items.is_empty() {
        let e = lbl("No context attached yet.", "sheet-empty");
        e.set_halign(gtk::Align::Center);
        list.append(&e);
        return;
    }

    for (index, item) in items.iter().enumerate() {
        let row = hbox(10);
        row.add_css_class("sheet-item");
        row.append(&icon("mail-attachment-symbolic", 15));

        let col = vbox(2);
        col.set_hexpand(true);
        col.append(&lbl(&item.name, "sheet-item-name"));
        col.append(&lbl(&fmt_size(item.size), "sheet-item-sub"));
        row.append(&col);

        let remove = icon_button("window-close-symbolic", "Remove", "pc-pin");
        remove.add_css_class("pinned");
        row.append(&remove);

        {
            let ui2 = ui.clone();
            let list2 = list.clone();
            let id2 = id.to_string();
            remove.connect_clicked(move |_| {
                {
                    let mut store = ui2.store.borrow_mut();
                    if let Some(p) = store.get_mut(&id2) {
                        if index < p.context.len() {
                            p.context.remove(index);
                            p.updated = model::now_millis();
                        }
                    }
                }
                save_store(&ui2);
                update_rail(&ui2);
                fill_context_list(&ui2, &list2, &id2);
            });
        }

        list.append(&row);
    }
}

fn fmt_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1_048_576 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

fn open_memory(ui: &Rc<Ui>) {
    let Some(id) = ui.current.borrow().clone() else {
        return;
    };
    let (win, body, foot) = sheet(ui, "Memory", 620);

    let hint = lbl(
        "Project memory is loaded into this project's working tree on every session, so ZO-AI already knows it before you type a word.",
        "field-hint",
    );
    hint.set_wrap(true);
    body.append(&hint);

    let entries = ui
        .store
        .borrow()
        .get(&id)
        .map(|p| p.memory.clone())
        .unwrap_or_default();

    if entries.is_empty() {
        let e = lbl(
            "No project memory yet. ZO-AI writes here as it learns what matters in this project.",
            "sheet-empty",
        );
        e.set_halign(gtk::Align::Center);
        e.set_wrap(true);
        e.set_justify(gtk::Justification::Center);
        body.append(&e);
    } else {
        for entry in entries {
            let row = hbox(10);
            row.add_css_class("sheet-item");
            row.append(&icon("preferences-other-symbolic", 15));
            let col = vbox(2);
            col.set_hexpand(true);
            col.append(&lbl(&entry.text, "sheet-item-name"));
            if !entry.when.is_empty() {
                col.append(&lbl(&entry.when, "sheet-item-sub"));
            }
            row.append(&col);
            body.append(&row);
        }
    }

    let close = secondary("Close");
    foot.append(&close);
    {
        let w = win.clone();
        close.connect_clicked(move |_| w.close());
    }

    win.present();
}

fn open_scheduled(ui: &Rc<Ui>) {
    let Some(id) = ui.current.borrow().clone() else {
        return;
    };
    let (win, body, foot) = sheet(ui, "Scheduled", 640);

    let list = vbox(8);
    body.append(&list);

    let what = gtk::Entry::new();
    what.set_placeholder_text(Some("e.g. summarise new commits and post them here"));
    what.add_css_class("field-input");
    body.append(&field("What should ZO-AI do?", &what));

    let when = gtk::Entry::new();
    when.set_placeholder_text(Some("2026-11-05 18:00"));
    when.add_css_class("field-input");

    let repeat = gtk::DropDown::from_strings(&["Once", "Daily", "Weekly"]);
    repeat.add_css_class("field-input");

    let when_box = vbox(7);
    when_box.append(&lbl("When", "field-hint"));
    when_box.append(&when);

    let repeat_box = vbox(7);
    repeat_box.append(&lbl("Repeat", "field-hint"));
    repeat_box.append(&repeat);

    let row2 = hbox(10);
    when_box.set_hexpand(true);
    repeat_box.set_hexpand(true);
    row2.append(&when_box);
    row2.append(&repeat_box);
    body.append(&row2);

    let hint = lbl(
        "You can also just ask ZO-AI in this project — “schedule this every Monday” — and it lands here.",
        "field-hint",
    );
    hint.set_wrap(true);
    body.append(&hint);

    let cancel = secondary("Cancel");
    let save = primary("Save");
    foot.append(&cancel);
    foot.append(&save);

    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }

    {
        let ui2 = ui.clone();
        let list2 = list.clone();
        let what2 = what.clone();
        let when2 = when.clone();
        let repeat2 = repeat.clone();
        let id_save = id.clone();
        save.connect_clicked(move |_| {
            let text = what2.text().to_string();
            if text.trim().is_empty() {
                what2.grab_focus();
                return;
            }
            let repeat_value = match repeat2.selected() {
                1 => "daily",
                2 => "weekly",
                _ => "once",
            };
            {
                let mut store = ui2.store.borrow_mut();
                if let Some(p) = store.get_mut(&id_save) {
                    p.schedules.push(Schedule {
                        text: text.clone(),
                        when: when2.text().to_string(),
                        repeat: repeat_value.to_string(),
                    });
                    p.updated = model::now_millis();
                }
            }
            save_store(&ui2);
            update_rail(&ui2);
            what2.set_text("");
            fill_sched_list(&ui2, &list2, &id_save);
        });
    }

    fill_sched_list(ui, &list, &id);
    win.present();
}

fn fill_sched_list(ui: &Rc<Ui>, list: &gtk::Box, id: &str) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let items: Vec<Schedule> = ui
        .store
        .borrow()
        .get(id)
        .map(|p| p.schedules.clone())
        .unwrap_or_default();

    if items.is_empty() {
        let e = lbl("Nothing scheduled in this project yet.", "sheet-empty");
        e.set_halign(gtk::Align::Center);
        list.append(&e);
        return;
    }

    for (index, item) in items.iter().enumerate() {
        let row = hbox(10);
        row.add_css_class("sheet-item");
        row.append(&icon("alarm-symbolic", 15));

        let col = vbox(2);
        col.set_hexpand(true);
        col.append(&lbl(&item.text, "sheet-item-name"));
        let mut meta = item.when.clone();
        if item.repeat != "once" {
            if !meta.is_empty() {
                meta.push_str(" · ");
            }
            meta.push_str(&item.repeat);
        }
        if !meta.is_empty() {
            col.append(&lbl(&meta, "sheet-item-sub"));
        }
        row.append(&col);

        let remove = icon_button("window-close-symbolic", "Remove", "pc-pin");
        remove.add_css_class("pinned");
        row.append(&remove);

        {
            let ui2 = ui.clone();
            let list2 = list.clone();
            let id2 = id.to_string();
            remove.connect_clicked(move |_| {
                {
                    let mut store = ui2.store.borrow_mut();
                    if let Some(p) = store.get_mut(&id2) {
                        if index < p.schedules.len() {
                            p.schedules.remove(index);
                            p.updated = model::now_millis();
                        }
                    }
                }
                save_store(&ui2);
                update_rail(&ui2);
                fill_sched_list(&ui2, &list2, &id2);
            });
        }

        list.append(&row);
    }
}

fn open_new_project(ui: &Rc<Ui>) {
    let (win, body, foot) = sheet(ui, "Create a project", 600);

    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("Name your project"));
    name.add_css_class("field-input");

    let goal = gtk::TextView::new();
    goal.set_wrap_mode(gtk::WrapMode::WordChar);
    goal.add_css_class("field-input");
    let goal_scroll = gtk::ScrolledWindow::builder()
        .child(&goal)
        .min_content_height(90)
        .build();

    let context = gtk::Button::with_label("Add context");
    context.add_css_class("btn");
    context.set_halign(gtk::Align::Start);

    body.append(&field("What are you working on?", &name));
    body.append(&field("What are you trying to achieve?", &goal_scroll));
    body.append(&field("Context", &context));

    let cancel = secondary("Cancel");
    let create = primary("Create project");
    create.set_sensitive(false);
    foot.append(&cancel);
    foot.append(&create);

    {
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
    }

    {
        let create2 = create.clone();
        name.connect_changed(move |e| {
            create2.set_sensitive(!e.text().trim().is_empty());
        });
    }

    {
        let ui2 = ui.clone();
        let w = win.clone();
        let name2 = name.clone();
        let goal2 = goal.clone();
        create.connect_clicked(move |_| {
            let n = name2.text().trim().to_string();
            if n.is_empty() {
                return;
            }
            let buffer = goal2.buffer();
            let g = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            let id = ui2.store.borrow_mut().add(n, g);
            save_store(&ui2);
            refresh_projects(&ui2);
            w.close();
            open_project(&ui2, &id);
        });
    }

    win.present();
}

fn open_placeholder(ui: &Rc<Ui>, title: &str, text: &str) {
    ui.stack.set_visible_child_name("placeholder");
    if let Some(w) = ui.stack.child_by_name("placeholder") {
        if let Some(bx) = w.downcast_ref::<gtk::Box>() {
            while let Some(c) = bx.first_child() {
                bx.remove(&c);
            }
            let head = lbl(title, "page-title");
            head.set_margin_top(34);
            head.set_margin_start(44);
            bx.append(&head);

            let empty = vbox(8);
            empty.add_css_class("empty-box");
            empty.set_halign(gtk::Align::Center);
            empty.set_valign(gtk::Align::Center);
            empty.set_vexpand(true);
            let t = lbl("Nothing here yet", "empty-title");
            t.set_halign(gtk::Align::Center);
            let d = lbl(text, "empty-text");
            d.set_halign(gtk::Align::Center);
            empty.append(&t);
            empty.append(&d);
            bx.append(&empty);
        }
    }
}

// ---------------------------------------------------------------- build

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

thread_local! {
    // the open main window, its page stack and the chat box; empty while the app only waits in the background
    static MAIN: RefCell<Option<(adw::ApplicationWindow, gtk::Stack, composer::Prompt)>> = const { RefCell::new(None) };
}

/// Bring up the main window (building it if it was closed), optionally with text already in the chat box.
fn show_main(text: &str) {
    let open = MAIN.with(|m| m.borrow().clone());
    if let Some((win, stack, prompt)) = open {
        stack.set_visible_child_name("chat");
        if !text.is_empty() {
            prompt.set_text(text);
        }
        win.present();
        prompt.grab_focus();
        return;
    }
    let Some(app) = gtk::gio::Application::default().and_then(|a| a.downcast::<adw::Application>().ok()) else { return };
    build_ui(&app);
    if let Some((_, _, prompt)) = MAIN.with(|m| m.borrow().clone()) {
        if !text.is_empty() {
            prompt.set_text(text);
        }
        prompt.grab_focus();
    }
}

fn build_ui(app: &adw::Application) {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("ZO-AI")
        .default_width(1240)
        .default_height(820)
        .build();
    window.add_css_class("zo-root");

    // ---------- sidebar ----------
    let nav_new = nav_button("list-add-symbolic", "New", true, None);
    let nav_search = nav_button("system-search-symbolic", "Search", false, None);
    let nav_projects = nav_button("view-grid-symbolic", "Projects", false, None);
    let nav_artifacts = nav_button("view-list-symbolic", "Artifacts", false, None);
    let nav_scheduled = nav_button("alarm-symbolic", "Scheduled", false, None);

    let sidebar = vbox(2);
    sidebar.add_css_class("sidebar");
    sidebar.set_size_request(268, -1);
    let brand = hbox(10);
    brand.set_margin_start(8);
    brand.append(&mark(26));
    brand.append(&lbl("ZO-AI", "wordmark"));
    sidebar.append(&brand);
    sidebar.append(&nav_new);
    sidebar.append(&nav_search);
    sidebar.append(&nav_projects);
    sidebar.append(&nav_artifacts);
    sidebar.append(&nav_scheduled);

    // pinned projects and recent chats, scrolling when there are many
    let lists = vbox(1);
    let pinned_label = lbl("Pinned", "side-label");
    let pinned_box = vbox(1);
    let recents_label = lbl("Recents", "side-label");
    let recents_box = vbox(1);
    lists.append(&pinned_label);
    lists.append(&pinned_box);
    lists.append(&recents_label);
    lists.append(&recents_box);
    let lists_scroll = gtk::ScrolledWindow::new();
    lists_scroll.set_child(Some(&lists));
    lists_scroll.set_vexpand(true);
    lists_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    sidebar.append(&lists_scroll);

    // who is signed in, and the way to the model settings
    let side_foot = hbox(10);
    side_foot.add_css_class("side-foot");
    let user = glib::user_name().to_string_lossy().into_owned();
    let initial = user.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "Z".into());
    let avatar = lbl(&initial, "avatar");
    avatar.set_halign(gtk::Align::Center);
    avatar.set_valign(gtk::Align::Center);
    avatar.set_xalign(0.5);
    avatar.set_width_chars(2);
    side_foot.append(&avatar);
    let who = vbox(0);
    who.set_hexpand(true);
    who.set_valign(gtk::Align::Center);
    who.append(&lbl(&user, "side-user"));
    side_foot.append(&who);
    let side_settings = icon_button("emblem-system-symbolic", "Model, key and address", "icon-btn");
    side_foot.append(&side_settings);
    sidebar.append(&side_foot);

    let reveal = gtk::Revealer::new();
    reveal.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    reveal.set_transition_duration(180);
    reveal.set_reveal_child(true);
    reveal.set_child(Some(&sidebar));

    // ---------- top bar ----------
    let toggle = icon_button("sidebar-show-symbolic", "Show or hide the sidebar", "icon-btn");

    let topbar = hbox(10);
    topbar.add_css_class("topbar");
    topbar.append(&toggle);
    let top_title = lbl("", "top-title");
    top_title.set_hexpand(true);
    topbar.append(&top_title);

    // ---------- stack ----------
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(120);
    stack.set_vexpand(true);

    // ---------- chat view ----------
    let chat = vbox(0);
    chat.set_valign(gtk::Align::Center);
    chat.set_halign(gtk::Align::Fill);

    let hero = hbox(14);
    hero.set_halign(gtk::Align::Center);
    hero.set_margin_bottom(28);
    hero.append(&mark(52));
    hero.append(&lbl(&chat::greeting(), "hero-title"));
    chat.append(&hero);

    let composer_wrap = vbox(6);
    composer_wrap.set_halign(gtk::Align::Center);
    composer_wrap.set_size_request(720, -1);
    composer_wrap.set_margin_start(24);
    composer_wrap.set_margin_end(24);

    // the card: the text on top, the controls under it
    let composer = vbox(0);
    composer.add_css_class("composer");
    let prompt = composer::Prompt::new("How can I help you today?");
    composer.append(prompt.widget());
    let tools = hbox(4);
    let plus = icon_button("list-add-symbolic", "Add a file or a screenshot", "tool-btn");
    tools.append(&plus);
    let tools_spacer = gtk::Box::new(Orientation::Horizontal, 0);
    tools_spacer.set_hexpand(true);
    tools.append(&tools_spacer);
    let send_btn = icon_button("go-up-symbolic", "Send", "tool-btn send");
    tools.append(&send_btn);
    composer.append(&tools);
    composer_wrap.append(&composer);

    // under the card: which model, which permission mode
    let pills = hbox(2);
    pills.set_halign(gtk::Align::End);
    let (model_btn, model_label) = chat::pill_button(&chat::pill_text(&zo_core::ai::Selection::load()), true);
    let (perm_btn, perm_label) = chat::pill_button(zo_core::Mode::load().name(), false);
    pills.append(&model_btn);
    pills.append(&perm_btn);
    composer_wrap.append(&pills);
    let note = lbl("ZO-AI can make mistakes. Check what it did.", "foot-note");
    note.set_halign(gtk::Align::Center);
    note.set_margin_top(6);
    composer_wrap.append(&note);

    chat.append(&composer_wrap);
    let chat_ctl = chat::Ctl::new(&chat, &hero);
    stack.add_named(&chat, Some("chat"));

    // ---------- projects view ----------
    let projects_page = vbox(0);

    let head = hbox(8);
    head.set_margin_top(34);
    head.set_margin_start(44);
    head.set_margin_end(44);
    head.set_margin_bottom(26);
    head.append(&lbl("Projects", "page-title"));

    let head_spacer = gtk::Box::new(Orientation::Horizontal, 0);
    head_spacer.set_hexpand(true);
    head.append(&head_spacer);

    let search_entry = gtk::Entry::new();
    search_entry.set_placeholder_text(Some("Search projects"));
    search_entry.add_css_class("field-input");
    search_entry.set_visible(false);
    head.append(&search_entry);

    let search_btn = icon_button("system-search-symbolic", "Search", "icon-btn");
    head.append(&search_btn);

    let sort_btn = gtk::MenuButton::new();
    sort_btn.set_icon_name("view-sort-descending-symbolic");
    sort_btn.add_css_class("icon-btn");
    sort_btn.set_has_frame(false);
    head.append(&sort_btn);

    let new_btn = gtk::Button::with_label("New project");
    new_btn.add_css_class("page-btn");
    new_btn.add_css_class("primary");
    head.append(&new_btn);

    projects_page.append(&head);

    let proj_list = vbox(0);
    proj_list.set_margin_start(44);
    proj_list.set_margin_end(44);
    proj_list.set_margin_bottom(44);

    let scroll = gtk::ScrolledWindow::builder()
        .child(&proj_list)
        .vexpand(true)
        .build();
    projects_page.append(&scroll);
    stack.add_named(&projects_page, Some("projects"));

    // ---------- project workspace ----------
    let project_page = hbox(0);

    let left = vbox(0);
    left.set_hexpand(true);
    left.set_margin_top(26);
    left.set_margin_start(38);
    left.set_margin_end(38);
    left.set_margin_bottom(30);

    let pv_title = lbl("Project", "pv-title");
    pv_title.set_margin_bottom(20);
    left.append(&pv_title);

    let pv_wrap = vbox(6);
    pv_wrap.set_halign(gtk::Align::Fill);
    let pv_composer = vbox(0);
    pv_composer.add_css_class("composer");
    let pv_prompt = composer::Prompt::new("New session");
    pv_composer.append(pv_prompt.widget());
    let pv_tools = hbox(4);
    let pv_plus = icon_button("list-add-symbolic", "Add a file or a folder", "tool-btn");
    pv_tools.append(&pv_plus);
    let pv_tools_spacer = gtk::Box::new(Orientation::Horizontal, 0);
    pv_tools_spacer.set_hexpand(true);
    pv_tools.append(&pv_tools_spacer);
    let pv_send = icon_button("go-up-symbolic", "Send", "tool-btn send");
    pv_tools.append(&pv_send);
    pv_composer.append(&pv_tools);
    pv_wrap.append(&pv_composer);

    let pv_meta = hbox(2);
    pv_meta.set_halign(gtk::Align::End);
    let (pv_model, pv_model_label) = chat::pill_button(&chat::pill_text(&zo_core::ai::Selection::load()), true);
    let (pv_perm, pv_perm_label) = chat::pill_button(zo_core::Mode::load().name(), false);
    pv_meta.append(&pv_model);
    pv_meta.append(&pv_perm);
    pv_wrap.append(&pv_meta);
    left.append(&pv_wrap);

    let hint_box = vbox(10);
    hint_box.set_valign(gtk::Align::Center);
    hint_box.set_vexpand(true);
    hint_box.set_halign(gtk::Align::Center);
    hint_box.set_margin_top(40);
    let bubble = icon("chat-message-new-symbolic", 30);
    bubble.add_css_class("pv-hint");
    hint_box.append(&bubble);
    let pv_hint = lbl(
        "ZO-AI remembers the same knowledge every time you talk to it in this project.",
        "pv-hint",
    );
    pv_hint.set_halign(gtk::Align::Center);
    pv_hint.set_wrap(true);
    pv_hint.set_justify(gtk::Justification::Center);
    pv_hint.set_max_width_chars(70);
    hint_box.append(&pv_hint);
    left.append(&hint_box);

    project_page.append(&left);

    let rail = vbox(0);
    rail.add_css_class("pv-rail");
    rail.set_size_request(296, -1);

    let (r_instr, a_instr) = rail_button("text-x-generic-symbolic", "Instructions");
    let (r_ctx, a_ctx) = rail_button("mail-attachment-symbolic", "Context");
    let (r_mem, a_mem) = rail_button("preferences-other-symbolic", "Memory");
    a_mem.set_text("View");
    let (r_sch, a_sch) = rail_button("alarm-symbolic", "Scheduled");

    rail.append(&r_instr);
    rail.append(&r_ctx);
    rail.append(&r_mem);
    rail.append(&r_sch);

    let rail_spacer = gtk::Box::new(Orientation::Vertical, 0);
    rail_spacer.set_vexpand(true);
    rail.append(&rail_spacer);
    rail.append(&lbl("Private", "rail-note"));

    project_page.append(&rail);
    stack.add_named(&project_page, Some("project"));

    // ---------- search view ----------
    let search_page = vbox(0);
    let sp_head = hbox(8);
    sp_head.set_margin_top(34);
    sp_head.set_margin_start(44);
    sp_head.set_margin_end(44);
    sp_head.set_margin_bottom(18);
    sp_head.append(&lbl("Search", "page-title"));
    search_page.append(&sp_head);
    let search_input = gtk::Entry::new();
    search_input.set_placeholder_text(Some("Search your chats and projects"));
    search_input.add_css_class("search-input");
    search_input.set_margin_start(44);
    search_input.set_margin_end(44);
    search_input.set_margin_bottom(14);
    search_page.append(&search_input);
    let search_results = vbox(8);
    search_results.set_margin_start(44);
    search_results.set_margin_end(44);
    search_results.set_margin_bottom(30);
    let search_scroll = gtk::ScrolledWindow::new();
    search_scroll.set_child(Some(&search_results));
    search_scroll.set_vexpand(true);
    search_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    search_page.append(&search_scroll);
    stack.add_named(&search_page, Some("search"));

    // ---------- placeholder view ----------
    let placeholder = vbox(0);
    stack.add_named(&placeholder, Some("placeholder"));

    // ---------- assemble ----------
    let main_col = vbox(0);
    main_col.set_hexpand(true);
    main_col.append(&topbar);
    main_col.append(&stack);

    let app_row = hbox(0);
    app_row.append(&reveal);
    app_row.append(&main_col);
    window.set_content(Some(&app_row));

    let ui = Rc::new(Ui {
        window: window.clone(),
        stack: stack.clone(),
        reveal: reveal.clone(),
        pinned: Cell::new(true),
        store: RefCell::new(Store::load()),
        current: RefCell::new(None),
        sort_mode: Cell::new(0),
        query: RefCell::new(String::new()),
        nav_new: nav_new.clone(),
        nav_search: nav_search.clone(),
        nav_projects: nav_projects.clone(),
        nav_artifacts: nav_artifacts.clone(),
        nav_scheduled: nav_scheduled.clone(),
        proj_list,
        search_entry,
        pv_title,
        pv_prompt,
        pv_hint,
        model_label,
        perm_label,
        pv_model_label,
        pv_perm_label,
        top_title,
        pinned_label,
        pinned_box,
        recents_label,
        recents_box,
        side_settings,
        chat: chat_ctl.clone(),
        search_input: search_input.clone(),
        search_results: search_results.clone(),
        rail_instructions: a_instr,
        rail_context: a_ctx,
        rail_memory: a_mem,
        rail_scheduled: a_sch,
    });

    // ---------- chat and model ----------
    {
        let (c, p, ui2) = (chat_ctl.clone(), prompt.clone(), ui.clone());
        send_btn.connect_clicked(move |_| {
            if c.is_busy() {
                c.stop();
                return;
            }
            c.submit(&p);
            ui2.top_title.set_text(&c.title());
        });
        // while it works the send button turns into a stop button
        let sb = send_btn.clone();
        chat_ctl.on_busy(Rc::new(move |busy| {
            sb.set_icon_name(if busy { "media-playback-stop-symbolic" } else { "go-up-symbolic" });
            sb.set_tooltip_text(Some(if busy { "Stop" } else { "Send" }));
        }));
        let (c, p, ui2) = (chat_ctl.clone(), prompt.clone(), ui.clone());
        prompt.connect_send(move || {
            c.submit(&p);
            ui2.top_title.set_text(&c.title());
        });
        let ui2 = ui.clone();
        model_btn.connect_clicked(move |b| chat::show_model_popover(&ui2, b));
        let ui2 = ui.clone();
        perm_btn.connect_clicked(move |b| chat::show_perm_popover(&ui2, b));
        let p = prompt.clone();
        plus.connect_clicked(move |b| chat::show_plus_popover(&p, b));
        let ui2 = ui.clone();
        ui.side_settings.connect_clicked(move |_| chat::models_sheet(&ui2));
        let c = chat_ctl.clone();
        nav_new.connect_clicked(move |_| c.new_conversation());
    }

    // ---------- sidebar behaviour ----------
    {
        let ui2 = ui.clone();
        toggle.connect_clicked(move |_| {
            let pinned = !ui2.pinned.get();
            ui2.pinned.set(pinned);
            ui2.reveal.set_reveal_child(pinned);
        });
    }

    {
        let reveal2 = reveal.clone();
        let ui2 = ui.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter(move |_, _, _| {
            if !ui2.pinned.get() {
                reveal2.set_reveal_child(true);
            }
        });
        toggle.add_controller(motion);
    }

    {
        let reveal2 = reveal.clone();
        let ui2 = ui.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter({
            let reveal3 = reveal2.clone();
            move |_, _, _| reveal3.set_reveal_child(true)
        });
        motion.connect_leave(move |_| {
            if !ui2.pinned.get() {
                let reveal3 = reveal2.clone();
                glib::timeout_add_local_once(Duration::from_millis(160), move || {
                    reveal3.set_reveal_child(false);
                });
            }
        });
        sidebar.add_controller(motion);
    }

    // ---------- navigation ----------
    for (btn, key) in [
        (&nav_new, "new"),
        (&nav_search, "search"),
        (&nav_projects, "projects"),
        (&nav_artifacts, "artifacts"),
        (&nav_scheduled, "scheduled"),
    ] {
        let ui2 = ui.clone();
        let key = key.to_string();
        btn.connect_clicked(move |_| match key.as_str() {
            "projects" => {
                refresh_projects(&ui2);
                ui2.stack.set_visible_child_name("projects");
                set_nav(&ui2, "projects");
            }
            "search" => {
                ui2.stack.set_visible_child_name("search");
                set_nav(&ui2, "search");
                refresh_search(&ui2);
                ui2.search_input.grab_focus();
            }
            "new" => {
                ui2.stack.set_visible_child_name("chat");
                set_nav(&ui2, "new");
            }
            "artifacts" => {
                open_placeholder(
                    &ui2,
                    "Artifacts",
                    "This area fills up as ZO-AI produces things for you.",
                );
                set_nav(&ui2, "artifacts");
            }
            _ => {
                open_placeholder(
                    &ui2,
                    "Scheduled",
                    "Scheduled runs show up here once you create one.",
                );
                set_nav(&ui2, "scheduled");
            }
        });
    }

    // ---------- project page buttons ----------
    {
        let ui2 = ui.clone();
        new_btn.connect_clicked(move |_| open_new_project(&ui2));
    }

    {
        let ui2 = ui.clone();
        search_btn.connect_clicked(move |_| {
            let visible = !WidgetExt::is_visible(&ui2.search_entry);
            ui2.search_entry.set_visible(visible);
            if visible {
                ui2.search_entry.grab_focus();
            }
        });
    }

    {
        let entry = ui.search_entry.clone();
        let ui2 = ui.clone();
        entry.connect_changed(move |e| {
            *ui2.query.borrow_mut() = e.text().to_string().to_lowercase();
            refresh_projects(&ui2);
        });
    }

    // sort menu
    {
        let pop = gtk::Popover::new();
        pop.add_css_class("zo-popover");
        let body = vbox(0);
        let options = ["Last updated", "Date created", "Alphabetical", "Group by type"];
        for (index, name) in options.iter().enumerate() {
            let row = pop_row(
                None,
                name,
                None,
                if index == 0 { Some("✓") } else { None },
                true,
            );
            let ui2 = ui.clone();
            let pop2 = pop.clone();
            let marker = index as u8;
            row.connect_clicked(move |_| {
                ui2.sort_mode.set(marker);
                refresh_projects(&ui2);
                pop2.popdown();
            });
            body.append(&row);
            if index == 2 {
                let sep = gtk::Box::new(Orientation::Horizontal, 0);
                sep.add_css_class("pop-sep");
                body.append(&sep);
            }
        }
        pop.set_child(Some(&body));
        sort_btn.set_popover(Some(&pop));
    }

    // ---------- rail actions ----------
    {
        let ui2 = ui.clone();
        r_instr.connect_clicked(move |_| open_instructions(&ui2));
    }
    {
        let ui2 = ui.clone();
        r_ctx.connect_clicked(move |_| open_context(&ui2));
    }
    {
        let ui2 = ui.clone();
        r_mem.connect_clicked(move |_| open_memory(&ui2));
    }
    {
        let ui2 = ui.clone();
        r_sch.connect_clicked(move |_| open_scheduled(&ui2));
    }

    // ---------- project page: model, mode, files and sending ----------
    {
        let ui2 = ui.clone();
        pv_model.connect_clicked(move |b| chat::show_model_popover(&ui2, b));
        let ui2 = ui.clone();
        pv_perm.connect_clicked(move |b| chat::show_perm_popover(&ui2, b));
        let p = ui.pv_prompt.clone();
        pv_plus.connect_clicked(move |b| chat::show_plus_popover(&p, b));

        // a message written in a project starts a chat that knows the project
        let (ui2, ctl, main_prompt) = (ui.clone(), chat_ctl.clone(), prompt.clone());
        let send: Rc<dyn Fn()> = Rc::new(move || {
            let text = ui2.pv_prompt.text().trim().to_string();
            if text.is_empty() {
                return;
            }
            ui2.pv_prompt.set_text("");
            ctl.new_conversation();
            ctl.set_context(project_context(&ui2));
            ui2.stack.set_visible_child_name("chat");
            main_prompt.set_text(&text);
            ctl.submit(&main_prompt);
            ui2.top_title.set_text(&ctl.title());
        });
        let s = send.clone();
        pv_send.connect_clicked(move |_| s());
        let s = send.clone();
        ui.pv_prompt.connect_send(move || s());
    }

    {
        let ui2 = ui.clone();
        ui.search_input.connect_changed(move |_| refresh_search(&ui2));
    }
    refresh_sidebar(&ui);
    {
        let weak = Rc::downgrade(&ui);
        chat_ctl.on_saved(Rc::new(move || {
            if let Some(ui) = weak.upgrade() {
                refresh_recents(&ui);
            }
        }));
        refresh_recents(&ui);
    }

    // ---------- persistence on close ----------
    {
        let ui2 = ui.clone();
        window.connect_close_request(move |_| {
            save_store(&ui2);
            MAIN.with(|m| *m.borrow_mut() = None);
            glib::Propagation::Proceed
        });
    }

    refresh_projects(&ui);

    // Optional start view, used for verification: ZO_AI_START_VIEW=projects|project
    match std::env::var("ZO_AI_START_VIEW").as_deref() {
        Ok("projects") => {
            ui.stack.set_visible_child_name("projects");
            set_nav(&ui, "projects");
        }
        Ok("project") => {
            let first = ui.store.borrow().projects.first().map(|p| p.id.clone());
            if let Some(id) = first {
                open_project(&ui, &id);
            }
        }
        _ => {}
    }

    MAIN.with(|m| *m.borrow_mut() = Some((window.clone(), stack.clone(), prompt.clone())));
    window.present();

    // Headless verification: ZO_AI_SELFTEST=1 prints the real widget tree and exits.
    if std::env::var_os("ZO_AI_SELFTEST").is_some() {
        let w = window.clone();
        let ui2 = ui.clone();
        glib::timeout_add_local_once(Duration::from_millis(700), move || {
            println!("=== view: chat ===");
            walk_tree(w.upcast_ref(), 0);

            ui2.stack.set_visible_child_name("projects");
            refresh_projects(&ui2);
            println!("=== view: projects ===");
            walk_tree(w.upcast_ref(), 0);

            let first = ui2.store.borrow().projects.first().map(|p| p.id.clone());
            if let Some(id) = first {
                open_project(&ui2, &id);
                println!("=== view: project ===");
                walk_tree(w.upcast_ref(), 0);
            }

            use std::io::Write;
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        });
    }
}

fn walk_tree(widget: &gtk::Widget, depth: usize) {
    let type_name = widget.type_().name().to_string();

    let mut text = String::new();
    if let Some(l) = widget.downcast_ref::<gtk::Label>() {
        text = l.text().to_string();
    } else if let Some(e) = widget.downcast_ref::<gtk::Entry>() {
        text = format!(
            "entry placeholder=\"{}\"",
            e.placeholder_text().map(|s| s.to_string()).unwrap_or_default()
        );
    } else if let Some(b) = widget.downcast_ref::<gtk::Button>() {
        text = b.label().map(|s| s.to_string()).unwrap_or_default();
        if text.is_empty() {
            if let Some(t) = b.tooltip_text() {
                text = format!("[{}]", t);
            }
        }
    }

    if !text.is_empty() {
        println!("{:width$}{} :: {}", "", type_name, text, width = depth * 2);
    }

    let mut child = widget.first_child();
    while let Some(c) = child {
        walk_tree(&c, depth + 1);
        child = c.next_sibling();
    }
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).flags(gtk::gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    app.connect_startup(|_| {
        load_css();
        gtk::Window::set_default_icon_name("zo-ai");
    });
    app.connect_command_line(|app, cmd| {
        // first start: begin answering approval questions and stay alive after the window closes
        static FIRST: std::sync::Once = std::sync::Once::new();
        FIRST.call_once(|| {
            ask::start();
            let app2 = app.clone();
            shortcut::register(move || quick::toggle(app2.upcast_ref(), |text| show_main(text)));
            std::mem::forget(app.hold());
        });
        let args: Vec<String> = cmd.arguments().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        let has = |f: &str| args.iter().any(|a| a == f);
        if has("--background") {
            // autostart: nothing to show
        } else if has("--quick") {
            quick::toggle(app.upcast_ref(), |text| show_main(text));
        } else {
            show_main("");
        }
        0
    });
    app.run()
}
