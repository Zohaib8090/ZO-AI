//! One-click Claude Code login on this computer. The sign-in is the Claude Code program's own (`claude auth login`, run on a pseudo-terminal
//! by `zo_core::claude_login`): ZO-AI shows its link as a button, takes the code the person pastes, and shows who is signed in
//! (`claude auth status`). It never sees, stores or shows the login itself.

use crate::{hbox, lbl, primary, secondary, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;
use zo_core::claude_login as login;

/// The text for a status.
pub(crate) fn status_text(installed: bool, logged_in: bool, account: &str) -> String {
    if !installed {
        "Not installed. Install it with: npm install -g @anthropic-ai/claude-code, then press Log in.".into()
    } else if logged_in {
        format!("Logged in{}. The Claude Code program keeps the login; ZO-AI never sees it.", if account.is_empty() { String::new() } else { format!(" as {account}") })
    } else {
        "Not logged in. Press Log in: your browser opens the Claude sign-in, you paste the code back here.".into()
    }
}

/// The login window: a link button, a place for the code, the state. Closing it cancels a sign-in that is still running.
pub(crate) fn login_sheet(ui: &Rc<Ui>, on_done: impl Fn() + 'static) {
    let (win, body, foot) = crate::sheet(ui, "Log in to Claude Code", 480);
    let msg = lbl("Starting the sign-in…", "field-hint");
    msg.set_wrap(true);
    msg.set_xalign(0.0);
    let link = primary("Open the Claude sign-in");
    link.set_halign(gtk::Align::Start);
    link.set_visible(false);
    let code = gtk::Entry::new();
    code.add_css_class("field-input");
    code.set_placeholder_text(Some("Paste the code"));
    code.set_max_length(login::MAX_CODE as i32);
    let code_box = crate::field("The code the page shows", &code);
    code_box.set_visible(false);
    body.append(&msg);
    body.append(&link);
    body.append(&code_box);
    let cancel = secondary("Cancel");
    let go = primary("Continue");
    go.set_visible(false);
    foot.append(&cancel);
    foot.append(&go);
    {
        let w = win.clone();
        cancel.connect_clicked(move |_| {
            login::desktop_cancel();
            w.close();
        });
    }
    let url = Rc::new(std::cell::RefCell::new(String::new()));
    {
        let (url, window) = (url.clone(), ui.window.clone());
        link.connect_clicked(move |_| {
            let u = url.borrow().clone();
            // only the sign-in pages of Anthropic (already checked by the sign-in code), opened by the system's browser
            if login::find_auth_url(&u).is_some() {
                gtk::UriLauncher::new(&u).launch(Some(&window), gtk::gio::Cancellable::NONE, |_| {});
            }
        });
    }
    let send = {
        let (code, msg) = (code.clone(), msg.clone());
        Rc::new(move || {
            let c = code.text().to_string();
            code.set_text("");
            match login::desktop_submit(&c) {
                Ok(_) => msg.set_text("Checking the code…"),
                Err(e) => msg.set_text(&e),
            }
        })
    };
    {
        let send = send.clone();
        go.connect_clicked(move |_| send());
    }
    code.connect_activate(move |_| send());
    let paint = {
        let (msg, link, code_box, go, url) = (msg.clone(), link.clone(), code_box.clone(), go.clone(), url.clone());
        move |v: &login::View| {
            msg.set_text(&v.message);
            if let Some(u) = &v.url {
                *url.borrow_mut() = u.clone();
            }
            link.set_visible(v.state == login::State::Link && v.url.is_some());
            code_box.set_visible(v.state == login::State::Link);
            go.set_visible(v.state == login::State::Link);
        }
    };
    match login::desktop_start() {
        Ok(v) => paint(&v),
        Err(e) => msg.set_text(&e),
    }
    let (done_once, on_done) = (Cell::new(false), on_done);
    let (w2, cancel2) = (win.clone(), cancel.clone());
    glib::timeout_add_local(Duration::from_millis(500), move || {
        // the window was closed: a sign-in that is still waiting is given up
        if w2.scrim.parent().is_none() {
            login::desktop_cancel();
            return glib::ControlFlow::Break;
        }
        let v = login::desktop_view();
        paint(&v);
        if v.state.over() {
            cancel2.set_label("Close");
            if v.state == login::State::Done && !done_once.replace(true) {
                on_done();
            }
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
    win.present();
    code.grab_focus();
}

/// Fills a label with the current state, asking the program in the background (it takes a moment).
pub(crate) fn fill_status(label: &gtk::Label, login_btn: &gtk::Button, logout_btn: &gtk::Button) {
    let (label, login_btn, logout_btn) = (label.clone(), login_btn.clone(), logout_btn.clone());
    label.set_text("Checking…");
    glib::spawn_future_local(async move {
        let (installed, logged, who) = gtk::gio::spawn_blocking(login::desktop_status).await.unwrap_or((false, false, String::new()));
        label.set_text(&status_text(installed, logged, &who));
        login_btn.set_label(if logged { "Log in again" } else { "Log in" });
        login_btn.set_sensitive(installed);
        logout_btn.set_sensitive(installed && logged);
    });
}

/// The buttons: Log in, Log out, Check. `on_change` runs after a login or logout (to redraw the page). The second value refreshes the state
/// line (asks the program in the background); call it when the buttons are shown.
pub(crate) fn buttons(ui: &Rc<Ui>, label: &gtk::Label, on_change: impl Fn() + 'static) -> (gtk::Box, Rc<dyn Fn()>) {
    let bar = hbox(6);
    let login_btn = primary("Log in");
    let logout_btn = secondary("Log out");
    let check = secondary("Check");
    bar.append(&login_btn);
    bar.append(&logout_btn);
    bar.append(&check);
    let on_change = Rc::new(on_change);
    let refresh: Rc<dyn Fn()> = {
        let (label, lb, lo) = (label.clone(), login_btn.clone(), logout_btn.clone());
        Rc::new(move || fill_status(&label, &lb, &lo))
    };
    {
        let (ui, on_change, refresh) = (ui.clone(), on_change.clone(), refresh.clone());
        login_btn.connect_clicked(move |_| {
            let (oc, rf) = (on_change.clone(), refresh.clone());
            login_sheet(&ui, move || {
                rf();
                oc();
            });
        });
    }
    {
        let (label, on_change, refresh) = (label.clone(), on_change.clone(), refresh.clone());
        logout_btn.connect_clicked(move |b| {
            b.set_sensitive(false);
            let (label, on_change, refresh) = (label.clone(), on_change.clone(), refresh.clone());
            glib::spawn_future_local(async move {
                let r = gtk::gio::spawn_blocking(login::desktop_logout).await.unwrap_or_else(|_| Err("Could not log out.".into()));
                match r {
                    Err(e) => label.set_text(&e),
                    Ok(()) => {
                        refresh();
                        on_change();
                    }
                }
            });
        });
    }
    {
        let refresh = refresh.clone();
        check.connect_clicked(move |_| refresh());
    }
    (bar, refresh)
}

/// A small block for the model sheet: the state line and the buttons. The closure refreshes the state; the block starts hidden.
pub(crate) fn block(ui: &Rc<Ui>) -> (gtk::Box, Rc<dyn Fn()>) {
    let b = vbox(6);
    let state = lbl("", "field-hint");
    state.set_wrap(true);
    state.set_xalign(0.0);
    b.append(&state);
    let (bar, refresh) = buttons(ui, &state, || {});
    b.append(&bar);
    b.set_visible(false);
    (b, refresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_says_what_to_do() {
        assert!(status_text(false, false, "").contains("npm install"));
        assert!(status_text(true, false, "").starts_with("Not logged in"));
        let t = status_text(true, true, "me@example.org (max)");
        assert!(t.contains("me@example.org (max)") && t.contains("never sees"), "{t}");
        assert!(!status_text(true, true, "").contains(" as "));
    }
}
