//! The quick popup: a small window for a fast question or action, without opening the full app.
//! Opened by the shortcut (Copilot key or Super+C) through `zo-ai --quick`; Esc or clicking away closes it.

use adw::prelude::*;
use gtk4 as gtk;
use gtk::{gio, glib};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use zo_core::{approval::AppApprover, mcp, Mode};

thread_local! {
    static POPUP: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
}

/// Show the popup, or hide it if it is already showing (the shortcut works as a toggle).
pub fn toggle(app: &gtk::Application, open_full: impl Fn(&str) + 'static) {
    let existing = POPUP.with(|p| p.borrow().clone());
    if let Some(w) = existing {
        w.close();
        return;
    }

    let win = gtk::Window::builder()
        .application(app)
        .title("ZO-AI")
        .decorated(false)
        .resizable(false)
        .default_width(640)
        .build();
    win.add_css_class("zo-root");
    win.add_css_class("quick");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.add_css_class("quick-body");

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Ask ZO-AI, or run something on this computer…"));
    entry.add_css_class("composer-entry");
    entry.add_css_class("quick-entry");
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    top.append(&crate::mark(30));
    entry.set_hexpand(true);
    top.append(&entry);
    root.append(&top);

    let out = gtk::Label::new(None);
    out.set_xalign(0.0);
    out.set_wrap(true);
    out.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    out.set_selectable(true);
    out.set_max_width_chars(80);
    out.add_css_class("quick-out");
    out.set_visible(false);
    let scroll = gtk::ScrolledWindow::builder().child(&out).max_content_height(300).propagate_natural_height(true).build();
    scroll.set_visible(false);
    root.append(&scroll);

    let busy = Rc::new(Cell::new(false));
    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for (label, tool, args) in [
        ("System info", "zohara_system", serde_json::json!({})),
        ("Screenshot", "zohara_ss", serde_json::json!({})),
        ("Snapshots", "zohara_zohara_list", serde_json::json!({})),
        ("Processes", "zohara_process_list", serde_json::json!({})),
    ] {
        let b = gtk::Button::with_label(label);
        b.add_css_class("foot-chip");
        let (out, scroll, busy) = (out.clone(), scroll.clone(), busy.clone());
        b.connect_clicked(move |b| {
            busy.set(true);
            b.set_sensitive(false);
            out.set_text("Working…");
            out.set_visible(true);
            scroll.set_visible(true);
            let (out, scroll, b, busy) = (out.clone(), scroll.clone(), b.clone(), busy.clone());
            let args = args.clone();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || mcp::execute(tool, &args, Mode::load(), &AppApprover)).await;
                let text = match result {
                    Ok(Ok(t)) => t,
                    Ok(Err(e)) => format!("Could not do that: {e}"),
                    Err(_) => "The action crashed.".into(),
                };
                out.set_text(&text);
                scroll.set_visible(true);
                b.set_sensitive(true);
                busy.set(false);
            });
        });
        chips.append(&b);
    }
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    chips.append(&spacer);
    let open = gtk::Button::with_label("Open ZO-AI");
    open.add_css_class("foot-chip");
    chips.append(&open);
    root.append(&chips);
    win.set_child(Some(&root));

    // Enter or "Open ZO-AI": carry what was typed into the full app
    let open_full = std::rc::Rc::new(open_full);
    {
        let (win, open_full) = (win.clone(), open_full.clone());
        entry.connect_activate(move |e| {
            let text = e.text().to_string();
            win.close();
            open_full(&text);
        });
    }
    {
        let (win, open_full, entry) = (win.clone(), open_full.clone(), entry.clone());
        open.connect_clicked(move |_| {
            let text = entry.text().to_string();
            win.close();
            open_full(&text);
        });
    }

    let keys = gtk::EventControllerKey::new();
    {
        let win = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                win.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    win.add_controller(keys);

    // clicking away closes it (but not while an approval dialog has taken the focus a moment)
    let busy_c = busy.clone();
    win.connect_is_active_notify(move |w| {
        if !w.is_active() {
            let w = w.clone();
            let busy = busy_c.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                if !busy.get() && !w.is_active() && w.is_visible() {
                    w.close();
                }
            });
        }
    });
    win.connect_close_request(|_| {
        POPUP.with(|p| *p.borrow_mut() = None);
        glib::Propagation::Proceed
    });

    POPUP.with(|p| *p.borrow_mut() = Some(win.clone()));
    win.present();
    entry.grab_focus();
}
