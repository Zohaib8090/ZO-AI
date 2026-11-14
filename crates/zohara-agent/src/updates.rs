//! Update checks on the desktop (docs/UPDATES.md). Policy: NOTIFY ONLY. Zohara Agent asks the Zohara Agent server (which reads the GitHub
//! release; GitHub never sees this computer) and tells the person. It never downloads or installs anything: the Update button opens the
//! release page, or shows the pacman command.
//!
//! * Settings > About: version, "Check now", the result, "Check for updates automatically".
//! * While the app runs (also in the tray), every 30 minutes it looks whether the last automatic check was 12 hours ago; if so it asks,
//!   and shows ONE desktop notification per new version (the last version told about is kept in `update-state.json`).

use crate::settings::{background, no_control, row, section, wrapped};
use crate::{hbox, lbl, secondary, vbox, Ui};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use zo_core::update::{self, Status, Verdict};

thread_local! {
    /// The newest answer this session, so the About page shows it without asking again.
    static LAST: RefCell<Option<Status>> = const { RefCell::new(None) };
}

/// The release page is only opened when it is a GitHub release address.
pub(crate) fn release_url_ok(u: &str) -> bool {
    u.starts_with("https://github.com/") && !u.contains(char::is_whitespace)
}

/// Title and text of the one notification for a new version (None when there is nothing to say).
pub(crate) fn notification_text(st: &Status) -> Option<(String, String)> {
    let (Verdict::Available, Some(l)) = (&st.verdict, &st.latest) else { return None };
    let first = l.notes.lines().map(str::trim).find(|x| !x.is_empty()).unwrap_or("");
    let mut body = format!("Version {} is available (you have {}).", l.version, st.current);
    if !first.is_empty() {
        body.push(' ');
        body.extend(first.chars().take(120));
    }
    let title = if st.required { "Zohara Agent update required" } else { "Zohara Agent update available" };
    Some((title.to_string(), body))
}

/// The actions the notification calls: `app.zo-update-about` (Settings > About) and `app.zo-update-release` (the release page).
pub fn init(app: &adw::Application) {
    let about = gtk::gio::SimpleAction::new("zo-update-about", None);
    about.connect_activate(|_, _| crate::open_about());
    app.add_action(&about);
    let release = gtk::gio::SimpleAction::new("zo-update-release", Some(gtk::glib::VariantTy::STRING));
    release.connect_activate(|_, p| {
        let Some(u) = p.and_then(|p| p.get::<String>()) else { return };
        open_release(&u);
    });
    app.add_action(&release);
}

fn open_release(url: &str) {
    if !release_url_ok(url) {
        return;
    }
    let parent = gtk::gio::Application::default().and_then(|a| a.downcast::<gtk::Application>().ok()).and_then(|a| a.active_window());
    gtk::UriLauncher::new(url).launch(parent.as_ref(), gtk::gio::Cancellable::NONE, |_| {});
}

fn check_now() -> Result<Status, String> {
    update::check_server(&zo_core::login::server_url()?, update::VERSION)
}

/// Starts the periodic check. Call once, on the first start of the app.
pub fn start() {
    gtk::glib::timeout_add_seconds_local_once(120, tick);
    gtk::glib::timeout_add_seconds_local(30 * 60, || {
        tick();
        gtk::glib::ControlFlow::Continue
    });
}

fn tick() {
    let now = update::now_secs();
    if !update::check_due(&update::read_state(&update::state_path()), now) {
        return;
    }
    std::thread::spawn(move || {
        let r = check_now();
        gtk::glib::MainContext::default().invoke(move || {
            if let Ok(st) = r {
                update::remember_checked(update::now_secs());
                announce(&st);
                LAST.with(|l| *l.borrow_mut() = Some(st));
            }
        });
    });
}

/// One notification per new version.
fn announce(st: &Status) {
    let Some((title, body)) = notification_text(st) else { return };
    let Some(l) = &st.latest else { return };
    if !update::should_notify(st, update::last_notified().as_deref()) {
        return;
    }
    let Some(app) = gtk::gio::Application::default() else { return };
    let n = gtk::gio::Notification::new(&title);
    n.set_body(Some(&body));
    n.set_default_action("app.zo-update-about");
    n.add_button_with_target_value("Release page", "app.zo-update-release", Some(&l.release_url.to_variant()));
    app.send_notification(Some("zo-update"), &n);
    update::remember_notified(&l.version);
}

/// Settings > About: the "Updates" card.
pub(crate) fn about_updates(ui: &Rc<Ui>, content: &gtk::Box) {
    let (sec, card) = section("Updates");
    let check = secondary("Check now");
    let status_row = row("Check for updates", "Looks for a newer release.", Some(&check));
    let sub = find_sub_label(&status_row);
    card.append(&status_row);
    let detail = vbox(8);
    detail.set_margin_start(16);
    detail.set_margin_end(16);
    detail.set_margin_bottom(12);
    detail.set_visible(false);
    card.append(&detail);

    let auto = gtk::Switch::new();
    auto.set_active(update::auto_check_enabled());
    auto.connect_state_set(|_, on| {
        update::set_auto_check(on);
        gtk::glib::Propagation::Proceed
    });
    card.append(&row(
        "Check for updates automatically",
        "Looks every 12 hours while Zohara Agent runs and tells you once per new version. It never downloads or installs anything: you decide.",
        Some(&auto),
    ));
    card.append(&row("Privacy", "The check asks the Zohara Agent server for the newest release. Nothing about you or this computer is sent, and GitHub never sees your address.", no_control()));
    content.append(&sec);

    let show = {
        let (sub, detail, ui) = (sub.clone(), detail.clone(), ui.clone());
        Rc::new(move |r: &Result<Status, String>| {
            while let Some(c) = detail.first_child() {
                detail.remove(&c);
            }
            detail.set_visible(false);
            match r {
                Err(e) => sub.set_text(&format!("Could not check for updates: {e}.")),
                Ok(st) => {
                    sub.set_text(&st.summary());
                    if let (Verdict::Available, Some(l)) = (st.verdict, &st.latest) {
                        detail.set_visible(true);
                        if st.required {
                            detail.append(&wrapped("This version is no longer supported. Please update."));
                        }
                        if !l.notes.is_empty() {
                            detail.append(&wrapped(&l.notes));
                        }
                        let buttons = hbox(8);
                        let page = secondary("Update: open the release page");
                        let url = l.release_url.clone();
                        page.connect_clicked(move |_| open_release(&url));
                        buttons.append(&page);
                        if let Some(cmd) = update::pacman_command(l) {
                            let show_cmd = secondary("Show the pacman command");
                            let (ui2, pkg) = (ui.clone(), l.assets.pkg.clone().unwrap_or_default());
                            show_cmd.connect_clicked(move |_| command_dialog(&ui2, &pkg, &cmd));
                            buttons.append(&show_cmd);
                        }
                        detail.append(&buttons);
                    }
                }
            }
        })
    };
    // what the session already knows
    if let Some(st) = LAST.with(|l| l.borrow().clone()) {
        show(&Ok(st));
    }
    let (show2, check2) = (show.clone(), check.clone());
    check.connect_clicked(move |_| {
        check2.set_sensitive(false);
        sub.set_text("Checking...");
        let (show3, check3) = (show2.clone(), check2.clone());
        background(check_now, move |r| {
            check3.set_sensitive(true);
            if let Ok(st) = &r {
                update::remember_checked(update::now_secs());
                LAST.with(|l| *l.borrow_mut() = Some(st.clone()));
            }
            show3(&r);
        });
    });
}

fn find_sub_label(r: &gtk::Box) -> gtk::Label {
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

fn command_dialog(ui: &Rc<Ui>, pkg: &str, cmd: &str) {
    let body = format!("1. Download {pkg} from the release page.\n2. In the folder you saved it in, run:\n\n{cmd}\n\nZohara Agent does not run this for you.");
    let d = adw::AlertDialog::new(Some("Update on Arch Linux"), Some(&body));
    d.add_responses(&[("close", "Close"), ("copy", "Copy the command")]);
    d.set_default_response(Some("close"));
    d.set_close_response("close");
    let (cmd, w) = (cmd.to_string(), ui.window.clone());
    d.connect_response(None, move |_, id| {
        if id == "copy" {
            w.clipboard().set_text(&cmd);
        }
    });
    d.present(Some(&ui.window));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(current: &str, latest: serde_json::Value) -> Status {
        Status::from_api(current, &json!({"ok": true, "repo": "o/r", "latest": latest}))
    }

    #[test]
    fn only_github_release_pages_are_opened() {
        assert!(release_url_ok("https://github.com/Zohaib8090/zohara-agent/releases/tag/v0.2.0"));
        for bad in ["http://github.com/a/b", "https://evil.example/https://github.com/", "file:///etc/passwd", "https://github.com/a b", "javascript:alert(1)", ""] {
            assert!(!release_url_ok(bad), "{bad}");
        }
    }

    #[test]
    fn the_notification_names_both_versions_and_the_first_line_of_the_notes() {
        let st = status("0.1.0", json!({"version": "0.2.0", "notes": "\n  Update checks everywhere.\nMore text."}));
        let (title, body) = notification_text(&st).unwrap();
        assert_eq!(title, "Zohara Agent update available");
        assert_eq!(body, "Version 0.2.0 is available (you have 0.1.0). Update checks everywhere.");
        let req = status("0.1.0", json!({"version": "0.3.0", "min_supported": "0.2.0"}));
        assert_eq!(notification_text(&req).unwrap().0, "Zohara Agent update required");
    }

    #[test]
    fn nothing_is_announced_when_up_to_date_or_unknown() {
        assert!(notification_text(&status("0.2.0", json!({"version": "0.2.0"}))).is_none());
        assert!(notification_text(&status("0.1.0", json!(null))).is_none());
    }

    #[test]
    fn long_notes_are_cut_in_the_notification() {
        let st = status("0.1.0", json!({"version": "0.2.0", "notes": "x".repeat(500)}));
        let (_, body) = notification_text(&st).unwrap();
        assert!(body.len() < 200, "{}", body.len());
    }
}
