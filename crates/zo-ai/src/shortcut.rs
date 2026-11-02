//! The global shortcut. Plasma's shortcut service (kglobalacceld) is asked over D-Bus to bind
//! the Copilot key (Meta+Shift+F23) and Ctrl+Super+C to "Quick ask", and tells us when one is pressed.
//! Users can change or remove the keys in System Settings > Shortcuts > ZO-AI.

use gtk4::gio;
use gtk4::glib::{self, prelude::ToVariant, Variant};

const COMPONENT: &str = "zo-ai";
const ACTION: &str = "quick";

// Qt key codes: Ctrl+Meta+C and Meta+Shift+F23 (what the Copilot key sends)
const KEYS: [i32; 2] = [0x1400_0043, 0x1300_0046];

fn call(conn: &gio::DBusConnection, path: &str, iface: &str, method: &str, args: Variant) -> Result<Variant, glib::Error> {
    conn.call_sync(Some("org.kde.kglobalaccel"), path, iface, method, Some(&args), None, gio::DBusCallFlags::NONE, 3000, None::<&gio::Cancellable>)
}

pub fn register(on_press: impl Fn() + 'static) {
    if let Err(e) = try_register(on_press) {
        eprintln!("zo-ai: could not register the shortcut (not running KDE Plasma?): {e}");
    }
}

fn try_register(on_press: impl Fn() + 'static) -> Result<(), glib::Error> {
    let conn = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    let id: Vec<String> = [COMPONENT, ACTION, "ZO-AI", "Quick ask"].iter().map(|s| s.to_string()).collect();
    let iface = "org.kde.KGlobalAccel";

    call(&conn, "/kglobalaccel", iface, "doRegister", (id.clone(),).to_variant())?;
    // keep what the person chose in System Settings; only fill in our defaults when nothing is bound yet
    let current = call(&conn, "/kglobalaccel", iface, "shortcut", (id.clone(),).to_variant())?;
    let bound = current.child_value(0).n_children() > 0;
    if !bound {
        const SET_PRESENT: u32 = 2;
        const NO_AUTOLOADING: u32 = 4;
        const IS_DEFAULT: u32 = 8;
        call(&conn, "/kglobalaccel", iface, "setShortcut", (id.clone(), KEYS.to_vec(), SET_PRESENT | IS_DEFAULT | NO_AUTOLOADING).to_variant())?;
        call(&conn, "/kglobalaccel", iface, "setShortcut", (id, KEYS.to_vec(), SET_PRESENT | NO_AUTOLOADING).to_variant())?;
    }

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
                on_press();
            }
        },
    );
    // stays subscribed for the life of the app
    std::mem::forget(id);
    std::mem::forget(conn);
    Ok(())
}
