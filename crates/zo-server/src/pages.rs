//! The two plain pages /privacy and /terms (the Markdown files in docs/ wrapped in a little HTML), and the security headers.
//! Everything from the Markdown is escaped before any markup is added, so the text can never inject HTML or script.

pub const PRIVACY_MD: &str = include_str!("../../../docs/PRIVACY.md");
pub const TERMS_MD: &str = include_str!("../../../docs/TERMS.md");

/// The version of the privacy notice and the terms (their "Last updated" date). A person accepts a version; the server records it.
pub const TERMS_VERSION: &str = "2026-11-12";

/// What the owner fills in for the public pages (environment settings; the server does not start without them).
#[derive(Clone, Debug, Default)]
pub struct Legal {
    pub contact: String,
    pub region: String,
    pub country: String,
}

/// The pages' text with the owner's details in place of `{{contact}}`, `{{region}}`, `{{country}}` and `{{version}}`.
pub fn fill(md: &str, l: &Legal) -> String {
    md.replace("{{contact}}", &l.contact).replace("{{region}}", &l.region).replace("{{country}}", &l.country).replace("{{version}}", TERMS_VERSION)
}

/// The Content-Security-Policy of the web app: its own inline script and style, Google's fonts, and what Firebase sign-in needs.
pub const APP_CSP: &str = "default-src 'self'; \
script-src 'self' 'unsafe-inline' https://www.gstatic.com https://apis.google.com; \
style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
font-src https://fonts.gstatic.com data:; \
img-src 'self' data: blob: https:; \
connect-src 'self' https://identitytoolkit.googleapis.com https://securetoken.googleapis.com https://www.googleapis.com https://*.googleapis.com https://www.gstatic.com; \
frame-src https://*.firebaseapp.com https://accounts.google.com https://apis.google.com; \
object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// For the plain text pages: no script at all.
pub const PLAIN_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// `**bold**`, `` `code` `` and `[text](https://link)` inside one already escaped line.
fn inline(escaped: &str) -> String {
    let mut out = String::new();
    let mut rest = escaped;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("**") {
            if let Some(end) = r.find("**") {
                out.push_str(&format!("<strong>{}</strong>", inline(&r[..end])));
                rest = &r[end + 2..];
                continue;
            }
        }
        if let Some(r) = rest.strip_prefix('`') {
            if let Some(end) = r.find('`') {
                out.push_str(&format!("<code>{}</code>", &r[..end]));
                rest = &r[end + 1..];
                continue;
            }
        }
        if let Some(r) = rest.strip_prefix('[') {
            if let Some(mid) = r.find("](") {
                if let Some(end) = r[mid + 2..].find(')') {
                    let (label, url) = (&r[..mid], &r[mid + 2..mid + 2 + end]);
                    // escaped text cannot hold a quote or an angle bracket, so the address cannot leave the attribute
                    if url.starts_with("https://") {
                        out.push_str(&format!("<a href=\"{url}\" rel=\"noopener noreferrer\">{label}</a>"));
                        rest = &r[mid + 3 + end..];
                        continue;
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

pub fn markdown(md: &str) -> String {
    let mut out = String::new();
    let (mut para, mut list, mut quote): (Vec<String>, Vec<String>, Vec<String>) = (vec![], vec![], vec![]);
    let mut table: Vec<Vec<String>> = vec![];
    fn flush(out: &mut String, para: &mut Vec<String>, list: &mut Vec<String>, quote: &mut Vec<String>, table: &mut Vec<Vec<String>>) {
        if !para.is_empty() {
            out.push_str(&format!("<p>{}</p>\n", para.join(" ")));
            para.clear();
        }
        if !list.is_empty() {
            out.push_str("<ul>\n");
            for i in list.drain(..) {
                out.push_str(&format!("<li>{i}</li>\n"));
            }
            out.push_str("</ul>\n");
        }
        if !quote.is_empty() {
            out.push_str(&format!("<blockquote>{}</blockquote>\n", quote.join(" ")));
            quote.clear();
        }
        if !table.is_empty() {
            out.push_str("<table>\n");
            for (n, row) in table.drain(..).enumerate() {
                let tag = if n == 0 { "th" } else { "td" };
                out.push_str("<tr>");
                for c in row {
                    out.push_str(&format!("<{tag}>{c}</{tag}>"));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</table>\n");
        }
    }
    for raw in md.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        if t.is_empty() {
            flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
        } else if let Some(h) = t.strip_prefix("### ") {
            flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
            out.push_str(&format!("<h3>{}</h3>\n", inline(&esc(h))));
        } else if let Some(h) = t.strip_prefix("## ") {
            flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
            out.push_str(&format!("<h2>{}</h2>\n", inline(&esc(h))));
        } else if let Some(h) = t.strip_prefix("# ") {
            flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
            out.push_str(&format!("<h1>{}</h1>\n", inline(&esc(h))));
        } else if let Some(q) = t.strip_prefix('>') {
            if !(para.is_empty() && list.is_empty() && table.is_empty()) {
                flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
            }
            quote.push(inline(&esc(q.trim())));
        } else if t.starts_with('|') {
            if !(para.is_empty() && list.is_empty() && quote.is_empty()) {
                flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
            }
            let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
            if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
                continue;
            }
            table.push(cells.iter().map(|c| inline(&esc(c))).collect());
        } else if let Some(i) = t.strip_prefix("* ").or_else(|| t.strip_prefix("- ")) {
            if !(para.is_empty() && quote.is_empty() && table.is_empty()) {
                flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
            }
            list.push(inline(&esc(i)));
        } else {
            if !(list.is_empty() && quote.is_empty() && table.is_empty()) {
                flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
            }
            para.push(inline(&esc(t)));
        }
    }
    flush(&mut out, &mut para, &mut list, &mut quote, &mut table);
    out
}

/// A whole page.
pub fn page(title: &str, md: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>\n:root{{color-scheme:light dark}}body{{font:16px/1.6 system-ui,sans-serif;max-width:46rem;margin:0 auto;padding:1.5rem 1rem 4rem}}\ntable{{border-collapse:collapse;width:100%;font-size:.92rem}}td,th{{border:1px solid #8884;padding:.4rem .6rem;text-align:left;vertical-align:top}}blockquote{{margin:1rem 0;padding:.1rem 1rem;border-left:4px solid #8886}}code{{background:#8882;padding:.05rem .3rem;border-radius:4px}}nav{{margin-bottom:1.5rem}}\n</style></head><body>\n<nav><a href=\"/\">ZO-AI</a> &middot; <a href=\"/privacy\">Privacy</a> &middot; <a href=\"/terms\">Terms</a></nav>\n{}</body></html>\n",
        esc(title),
        markdown(md)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_from_the_text_becomes_markup() {
        let h = markdown("# <script>alert(1)</script>\n\nHello <img src=x onerror=alert(1)> **bold** `<b>`\n\n- item \"quoted\"\n\n[ok](https://example.com/a?b=1&c=2) [bad](javascript:alert(1)) [x](https://a.b\" onclick=\"y)");
        assert!(!h.contains("<script") && !h.contains("<img"));
        assert!(h.contains("&lt;script&gt;") && h.contains("<strong>bold</strong>") && h.contains("<code>&lt;b&gt;</code>"));
        assert!(h.contains("<a href=\"https://example.com/a?b=1&amp;c=2\""));
        assert!(!h.contains("href=\"javascript"), "only https links become links");
        assert!(!h.contains("\" onclick=\""), "an address cannot break out of its attribute: {h}");
    }

    #[test]
    fn headings_lists_tables_and_quotes() {
        let h = markdown("# T\n\n> note\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n* x\n* y\n\nplain\ntext");
        for want in ["<h1>T</h1>", "<blockquote>note</blockquote>", "<th>a</th>", "<td>2</td>", "<li>y</li>", "<p>plain text</p>"] {
            assert!(h.contains(want), "{want} in {h}");
        }
        assert!(!h.contains("---"));
    }

    #[test]
    fn the_real_pages_render() {
        let legal = Legal { contact: "me@example.org".into(), region: "Frankfurt, Germany".into(), country: "Germany".into() };
        for (t, md) in [("Privacy", PRIVACY_MD), ("Terms", TERMS_MD)] {
            let p = page(t, &fill(md, &legal));
            assert!(p.contains("<h1>") && p.len() > 2000 && !p.contains("<script"), "{t}");
            // nothing is left to fill in: no placeholder, no template banner
            assert!(!p.contains("{{") && !p.contains("[date]") && !p.contains("[contact") && !p.to_lowercase().contains("template"), "{t}");
            assert!(p.contains("me@example.org") && p.contains(TERMS_VERSION));
        }
        assert!(fill(PRIVACY_MD, &legal).contains("Frankfurt, Germany") && fill(TERMS_MD, &legal).contains("Germany"));
    }

    #[test]
    fn the_policy_allows_what_the_page_needs() {
        for want in ["https://www.gstatic.com", "https://apis.google.com", "https://fonts.googleapis.com", "https://fonts.gstatic.com", "https://identitytoolkit.googleapis.com", "https://securetoken.googleapis.com", "https://*.firebaseapp.com", "frame-ancestors 'none'", "object-src 'none'"] {
            assert!(APP_CSP.contains(want), "{want}");
        }
    }
}
