//! The installable web app: the manifest, the icons and the service worker (`web/sw.js`). Served without a login (a browser
//! fetches them before anyone is signed in). The service worker gets a version that is a hash of the page, so every new page is a
//! new worker and the app shows "Update available".

use crate::{header, send_response, App};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use tiny_http::{Request, Response};

const INDEX: &str = include_str!("../web/index.html");
const SW: &str = include_str!("../web/sw.js");
const MANIFEST: &str = include_str!("../web/manifest.webmanifest");

const ICONS: [(&str, &[u8]); 5] = [
    ("/icons/icon-192.png", include_bytes!("../web/icons/icon-192.png")),
    ("/icons/icon-512.png", include_bytes!("../web/icons/icon-512.png")),
    ("/icons/maskable-192.png", include_bytes!("../web/icons/maskable-192.png")),
    ("/icons/maskable-512.png", include_bytes!("../web/icons/maskable-512.png")),
    ("/apple-touch-icon.png", include_bytes!("../web/apple-touch-icon.png")),
];

pub fn serves(path: &str) -> bool {
    matches!(path, "/sw.js" | "/manifest.webmanifest") || ICONS.iter().any(|(p, _)| *p == path)
}

/// The worker's text with its version in it: a hash of everything it caches that is not an image, and of itself.
pub fn sw_text() -> &'static str {
    static T: OnceLock<String> = OnceLock::new();
    T.get_or_init(|| {
        let mut h = Sha256::new();
        for part in [INDEX, SW, MANIFEST] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        let v: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
        SW.replace("__VERSION__", &v)
    })
}

pub fn serve(app: &App, req: Request, path: &str) {
    let (body, ctype, cache): (Vec<u8>, &str, &str) = match path {
        "/sw.js" => (sw_text().as_bytes().to_vec(), "text/javascript; charset=utf-8", "no-cache"),
        "/manifest.webmanifest" => (MANIFEST.as_bytes().to_vec(), "application/manifest+json", "no-cache"),
        p => match ICONS.iter().find(|(n, _)| *n == p) {
            Some((_, b)) => (b.to_vec(), "image/png", "public, max-age=86400"),
            None => return crate::text(app, req, 404, "not found"),
        },
    };
    let mut r = Response::from_data(body).with_header(header("Content-Type", ctype)).with_header(header("Cache-Control", cache));
    if path == "/sw.js" {
        // the worker may control the whole site (it is served from /, so this is the default; saying it keeps it so behind a proxy)
        r.add_header(header("Service-Worker-Allowed", "/"));
    }
    send_response(app, req, r);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worker_has_a_version_and_the_manifest_is_valid() {
        let sw = sw_text();
        assert!(!sw.contains("__VERSION__") && sw.contains("const VERSION = \""));
        assert!(!sw.contains("/api/\"") || sw.contains("startsWith(\"/api/\")"), "the worker leaves /api/ alone");
        let m: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!((m["display"].as_str(), m["start_url"].as_str(), m["scope"].as_str()), (Some("standalone"), Some("/"), Some("/")));
        let icons = m["icons"].as_array().unwrap();
        for (size, purpose) in [("192x192", "any"), ("512x512", "any"), ("192x192", "maskable"), ("512x512", "maskable")] {
            assert!(icons.iter().any(|i| i["sizes"] == size && i["purpose"] == purpose), "{size} {purpose}");
        }
        for i in icons {
            assert!(ICONS.iter().any(|(p, b)| *p == i["src"].as_str().unwrap() && b.starts_with(b"\x89PNG")), "{i}");
        }
        assert!(INDEX.contains("rel=\"manifest\"") && INDEX.contains("apple-touch-icon") && INDEX.contains("theme-color"));
    }
}
