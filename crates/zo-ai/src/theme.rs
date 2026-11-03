//! Dark and light looks, both made from the one stylesheet: the light one swaps the palette.

use gtk4 as gtk;
use libadwaita as adw;
use std::cell::RefCell;

/// dark colour -> its light counterpart.
const MAP: &[(&str, &str)] = &[
    ("#0B0E14", "#FFFFFF"), // the text on the cyan send button
    ("#171717", "#F1EFE8"),
    ("#191919", "#FAF9F5"),
    ("#1C1C1C", "#F3F1EA"),
    ("#1F1F1F", "#F3F1EA"),
    ("#202020", "#FFFFFF"),
    ("#242424", "#F3F1EA"),
    ("#262626", "#FFFFFF"),
    ("#2A2A2A", "#E9E6DC"),
    ("#2B2B2B", "#E9E6DC"),
    ("#2E2E2E", "#EDEAE0"),
    ("#303030", "#FFFFFF"),
    ("#353535", "#DAD6C9"),
    ("#3A3A3A", "#D3CFC1"),
    ("#7C7C7C", "#8A877F"),
    ("#8C8C8C", "#7A776F"),
    ("#9A9A9A", "#6B6963"),
    ("#BDBDBD", "#55534E"),
    ("#D0D0D0", "#3D3C39"),
    ("#E4E4E4", "#2A2926"),
    ("#ECECEC", "#1F1E1D"),
    ("#FFFFFF", "#000000"),
    ("#5FD4D4", "#1FA5A5"),
    ("#7EE0E0", "#2BB8B8"),
];

/// Colours that look right in both themes.
const SAME: &[&str] = &["#22C55E", "#4C9AFF", "#C0392B", "#E5736A"];

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// The stylesheet with its palette swapped for the light one.
pub fn light(css: &str) -> String {
    let b = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    while i < css.len() {
        if b[i] == b'#' && i + 7 <= css.len() && b[i + 1..i + 7].iter().all(|c| is_hex(*c)) && !b.get(i + 7).is_some_and(|c| is_hex(*c)) {
            let tok = css[i..i + 7].to_uppercase();
            match MAP.iter().find(|(d, _)| *d == tok) {
                Some((_, l)) => out.push_str(l),
                None => out.push_str(&css[i..i + 7]),
            }
            i += 7;
        } else {
            let ch = css[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out.replace("alpha(white", "alpha(black")
}

fn file() -> std::path::PathBuf {
    zo_core::audit::config_dir().join("appearance")
}

/// "system", "dark" or "light".
pub fn choice() -> String {
    let c = std::fs::read_to_string(file()).unwrap_or_default().trim().to_string();
    if c == "dark" || c == "light" { c } else { "system".into() }
}

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

fn load_css() {
    let dark = adw::StyleManager::default().is_dark();
    let css = if dark { crate::CSS.to_string() } else { light(crate::CSS) };
    PROVIDER.with(|p| {
        let mut p = p.borrow_mut();
        let provider = p.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            }
            provider
        });
        provider.load_from_string(&css);
    });
}

/// Applies the saved choice: libadwaita's own widgets and our stylesheet.
pub fn apply() {
    let sm = adw::StyleManager::default();
    sm.set_color_scheme(match choice().as_str() {
        "dark" => adw::ColorScheme::ForceDark,
        "light" => adw::ColorScheme::ForceLight,
        _ => adw::ColorScheme::Default,
    });
    load_css();
}

/// Saves the choice and applies it at once.
pub fn set(name: &str) {
    let _ = std::fs::create_dir_all(zo_core::audit::config_dir());
    let _ = std::fs::write(file(), name);
    apply();
}

/// Once, at start: apply the choice, and follow the system when it changes.
pub fn init() {
    apply();
    adw::StyleManager::default().connect_dark_notify(|_| load_css());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_sheet_swaps_the_palette_and_the_overlays() {
        let css = "a { color: #ECECEC; background: #191919; border: 1px solid alpha(white, 0.06); box-shadow: 0 2px alpha(black, 0.3); } b { color: #5fd4d4; } c { color: #4C9AFF; }";
        let l = light(css);
        assert!(l.contains("color: #1F1E1D") && l.contains("background: #FAF9F5") && l.contains("alpha(black, 0.06)"));
        assert!(l.contains("#1FA5A5"), "lower-case colours are found too");
        assert!(l.contains("#4C9AFF"), "colours that suit both stay");
        assert!(!l.contains("#191919") && !l.contains("alpha(white"));
    }

    #[test]
    fn every_colour_in_the_stylesheet_has_a_light_one_or_is_marked_as_fine_in_both() {
        let css = include_str!("style.css");
        let mut missing = Vec::new();
        let b = css.as_bytes();
        for i in 0..css.len().saturating_sub(7) {
            if b[i] == b'#' && b[i + 1..i + 7].iter().all(|c| is_hex(*c)) && !b.get(i + 7).is_some_and(|c| is_hex(*c)) {
                let t = css[i..i + 7].to_uppercase();
                if !MAP.iter().any(|(d, _)| *d == t) && !SAME.contains(&t.as_str()) && !missing.contains(&t) {
                    missing.push(t);
                }
            }
        }
        assert!(missing.is_empty(), "add these to the light palette: {missing:?}");
    }
}
