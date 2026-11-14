//! The installable web app: the manifest, the icons and the service worker (`web/sw.js`). Served without a login (a browser
//! fetches them before anyone is signed in). The service worker gets a version that is a hash of the page, so every new page is a
//! new worker and the app shows "New version available - Reload".

use crate::{header, send_response, App};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use tiny_http::{Request, Response};

const INDEX: &str = include_str!("../web/index.html");
const SW: &str = include_str!("../web/sw.js");
const MANIFEST: &str = include_str!("../web/manifest.webmanifest");
/// The address the page files in the repository name (robots.txt, sitemap, canonical, og:url); the server swaps it for its own
/// address (ZO_PUBLIC_URL / RENDER_EXTERNAL_URL) when that is set, so a move to another host does not leave the old one in every page.
pub const DEFAULT_BASE: &str = "https://zohara-agent.duckdns.org";

fn base_of(app: &App) -> &str {
    app.public_url.as_deref().map(|u| u.trim_end_matches('/')).filter(|u| !u.is_empty()).unwrap_or(DEFAULT_BASE)
}

fn robots(base: &str) -> String {
    format!("User-agent: *\nAllow: /\nSitemap: {base}/sitemap.xml\n")
}

fn sitemap(base: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>{base}/</loc><changefreq>weekly</changefreq><priority>1.0</priority></url>
  <url><loc>{base}/privacy</loc><changefreq>monthly</changefreq><priority>0.5</priority></url>
  <url><loc>{base}/terms</loc><changefreq>monthly</changefreq><priority>0.5</priority></url>
</urlset>
"#
    )
}

/// The web build shown in Settings > About: the workspace version and the first characters of the hash that also versions the service worker.
pub fn web_build() -> &'static str {
    static B: OnceLock<String> = OnceLock::new();
    B.get_or_init(|| {
        let v = sw_text();
        let hash = v.split("const VERSION = \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
        format!("{} ({})", env!("CARGO_PKG_VERSION"), hash.chars().take(7).collect::<String>())
    })
}

/// The app page with this server's own address in its canonical, og: and JSON-LD tags, and its build in the About page.
pub fn index_for(app: &App) -> String {
    INDEX.replace(DEFAULT_BASE, base_of(app)).replace("__WEB_BUILD__", web_build())
}

const ICONS: [(&str, &[u8]); 6] = [
    ("/icons/icon-192.png",    include_bytes!("../web/icons/icon-192.png")),
    ("/icons/icon-512.png",    include_bytes!("../web/icons/icon-512.png")),
    ("/icons/maskable-192.png", include_bytes!("../web/icons/maskable-192.png")),
    ("/icons/maskable-512.png", include_bytes!("../web/icons/maskable-512.png")),
    ("/apple-touch-icon.png",  include_bytes!("../web/apple-touch-icon.png")),
    ("/logo.png",              include_bytes!("../web/logo.png")),
];

pub fn serves(path: &str) -> bool {
    matches!(path, "/sw.js" | "/manifest.webmanifest" | "/robots.txt" | "/sitemap.xml")
        || ICONS.iter().any(|(p, _)| *p == path)
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
        "/sw.js"              => (sw_text().as_bytes().to_vec(), "text/javascript; charset=utf-8", "no-cache"),
        "/manifest.webmanifest" => (MANIFEST.as_bytes().to_vec(), "application/manifest+json", "no-cache"),
        "/robots.txt"         => (robots(base_of(app)).into_bytes(), "text/plain; charset=utf-8", "no-cache"),
        "/sitemap.xml"        => (sitemap(base_of(app)).into_bytes(), "application/xml; charset=utf-8", "public, max-age=3600"),
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

    #[test]
    fn the_page_shows_its_build_and_offers_reload_when_a_new_worker_waits() {
        // __WEB_BUILD__ is replaced in what is served; the toast text and the About checks are in the page
        assert!(INDEX.contains("const WEB_BUILD = \"__WEB_BUILD__\""));
        let b = web_build();
        assert!(b.starts_with(env!("CARGO_PKG_VERSION")) && b.ends_with(')') && !b.contains("__"), "{b}");
        assert_eq!(b.rsplit('(').next().unwrap().trim_end_matches(')').len(), 7);
        for needle in ["New version available", "checkWebUpdate", "/api/update/latest", "function isNewerVersion", "swReg.waiting"] {
            assert!(INDEX.contains(needle), "{needle}");
        }
        assert!(!INDEX.contains("Update available"));
    }

    #[test]
    fn robots_points_to_sitemap_and_sitemap_has_key_urls() {
        let (r, s) = (robots(DEFAULT_BASE), sitemap(DEFAULT_BASE));
        assert!(r.contains("Sitemap:") && r.contains("sitemap.xml"));
        assert!(s.contains("<loc>https://zohara-agent.duckdns.org/</loc>"));
        assert!(s.contains("/privacy") && s.contains("/terms"));
        // another host: nothing names the old one
        let (r, s) = (robots("https://ai.example.org"), sitemap("https://ai.example.org"));
        assert!(r.contains("https://ai.example.org/sitemap.xml") && !r.contains("onrender") && !s.contains("onrender") && s.contains("https://ai.example.org/terms"));
        let moved = INDEX.replace(DEFAULT_BASE, "https://ai.example.org");
        assert!(!moved.contains("onrender.com") && moved.contains("rel=\"canonical\" href=\"https://ai.example.org/\""));
    }
}
