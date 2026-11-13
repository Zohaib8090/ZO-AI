//! The global shortcut. Plasma's shortcut service (kglobalacceld) is asked over D-Bus to bind
//! the Copilot key (Meta+Shift+F23) and Ctrl+Super+C to "Quick ask", and tells us when one is pressed.
//! Users can change or remove the keys in System Settings > Shortcuts > Zohara Agent.

use gtk4::gio;
use gtk4::glib::{self, prelude::ToVariant, Variant};

const COMPONENT: &str = "zohara-agent";
/// The components under the old names (Zohara Chat, then ZO-AI) and the titles they were registered with: the keys a person chose there
/// carry over once.
const OLD_COMPONENTS: [(&str, &str); 2] = [("zohara-chat", "Zohara Chat"), ("zo-ai", "ZO-AI")];
const ACTION: &str = "quick";

// Qt key codes: Ctrl+Meta+C, and Meta+Shift+F23 (what the Copilot key sends on the keyboard).
// The Copilot key cannot be made to work through Plasma's shortcuts yet: with the usual keyboard layout, F23 held with Shift or
// Meta is the "assistant" key, which Qt has no code for, and Plasma then reads it as Touchpad Off, which it keeps for itself
// ("Disable Touchpad"). Checked on 2026-11-08 by sending the real key events. Meta+Shift+F23 stays for layouts that deliver F23 as is.
const KEYS: [i32; 2] = [0x1400_0043, 0x1300_0046];
/// Keys that earlier versions of Zohara Agent set; a person who never changed them gets the current ones.
const OLD_DEFAULTS: [[i32; 3]; 1] = [[0x1100_0112, 0x1400_0043, 0x1300_0046]];

fn is_old_default(chosen: &[i32]) -> bool {
    let mut c = chosen.to_vec();
    c.sort_unstable();
    OLD_DEFAULTS.iter().any(|d| {
        let mut d = d.to_vec();
        d.sort_unstable();
        d == c
    })
}

/// The keys for the new component: its own when it has any, else the ones chosen under the newest old name that has any.
fn inherit(new_keys: &[i32], olds: &[Vec<i32>]) -> Vec<i32> {
    if !new_keys.is_empty() {
        return new_keys.to_vec();
    }
    olds.iter().find(|k| !k.is_empty()).cloned().unwrap_or_default()
}

fn call(conn: &gio::DBusConnection, path: &str, iface: &str, method: &str, args: Variant) -> Result<Variant, glib::Error> {
    conn.call_sync(Some("org.kde.kglobalaccel"), path, iface, method, Some(&args), None, gio::DBusCallFlags::NONE, 3000, None::<&gio::Cancellable>)
}

pub fn register(on_press: impl Fn() + 'static) {
    if let Err(e) = try_register(on_press) {
        eprintln!("zohara-agent: could not register the shortcut (not running KDE Plasma?): {e}");
    }
}

fn try_register(on_press: impl Fn() + 'static) -> Result<(), glib::Error> {
    let conn = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    let id: Vec<String> = [COMPONENT, ACTION, "Zohara Agent", "Quick ask"].iter().map(|s| s.to_string()).collect();
    let iface = "org.kde.KGlobalAccel";

    call(&conn, "/kglobalaccel", iface, "doRegister", (id.clone(),).to_variant())?;
    // Keep what the person chose in System Settings; only fill in our defaults when nothing is bound yet.
    const SET_PRESENT: u32 = 2;
    const NO_AUTOLOADING: u32 = 4;
    const IS_DEFAULT: u32 = 8;
    let current = call(&conn, "/kglobalaccel", iface, "shortcut", (id.clone(),).to_variant())?;
    let chosen: Vec<i32> = current.child_value(0).iter().filter_map(|k| k.get::<i32>()).collect();
    // the first start after a rename: the keys the person set under an old component name (the old entries stay)
    let chosen = if chosen.is_empty() {
        let olds: Vec<Vec<i32>> = OLD_COMPONENTS
            .iter()
            .map(|(component, title)| {
                let old_id: Vec<String> = [*component, ACTION, *title, "Quick ask"].iter().map(|s| s.to_string()).collect();
                call(&conn, "/kglobalaccel", iface, "shortcut", (old_id,).to_variant())
                    .map(|v| v.child_value(0).iter().filter_map(|k| k.get::<i32>()).collect())
                    .unwrap_or_default()
            })
            .collect();
        inherit(&chosen, &olds)
    } else {
        chosen
    };
    let reset = chosen.is_empty() || is_old_default(&chosen);
    if reset {
        call(&conn, "/kglobalaccel", iface, "setShortcut", (id.clone(), KEYS.to_vec(), SET_PRESENT | IS_DEFAULT | NO_AUTOLOADING).to_variant())?;
    }
    // Every start must mark the action as present with its keys. Plasma restores the saved keys by itself, but it only
    // grabs them (so the keys really do something) once the program has said the action is here; without this call the
    // shortcut looked set in System Settings and never fired after the first run.
    let keys = if reset { KEYS.to_vec() } else { chosen };
    call(&conn, "/kglobalaccel", iface, "setShortcut", (id, keys, SET_PRESENT | NO_AUTOLOADING).to_variant())?;

    let path = call(&conn, "/kglobalaccel", iface, "getComponent", (COMPONENT,).to_variant())?;
    let path = path.child_value(0).get::<String>().unwrap_or_default();
    let id = conn.signal_subscribe(
        Some("org.kde.kglobalaccel"),
        Some("org.kde.kglobalaccel.Component"),
        Some("globalShortcutPressed"),
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, params| {
            if params.child_value(1).get::<String>().as_deref() == Some(ACTION) {
                if zo_core::legacy::env_os("DEBUG").is_some() {
                    eprintln!("zohara-agent: the quick-ask shortcut was pressed");
                }
                on_press();
            }
        },
    );
    // stays subscribed for the life of the app
    std::mem::forget(id);
    std::mem::forget(conn);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_chosen_under_an_old_component_carry_over_once() {
        assert_eq!(inherit(&[], &[vec![7, 8], vec![]]), vec![7, 8], "nothing under the new name yet: the old choice is used");
        assert_eq!(inherit(&[1], &[vec![7, 8]]), vec![1], "the new name has its own keys: they win");
        assert_eq!(inherit(&[], &[vec![], vec![5]]), vec![5], "an install that only ever had the ZO-AI component");
        assert_eq!(inherit(&[], &[vec![7], vec![5]]), vec![7], "the Zohara Chat choice is newer than the ZO-AI one");
        assert!(inherit(&[], &[vec![], vec![]]).is_empty());
        assert!(inherit(&[], &[]).is_empty());
    }
}
