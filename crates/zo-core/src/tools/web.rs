//! Web search and page reading for the AI, with no key and no account: the search goes to DuckDuckGo's plain page, and a page is
//! fetched and reduced to its text. Only public http(s) addresses are read: nothing on this computer or the home network.

use super::text;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:125.0) Gecko/20100101 Firefox/125.0";

const PRIVATE_MSG: &str = "That address is on this computer or its own network, so it is not read.";

/// False for addresses on this computer or its own network (loopback, private, link-local, carrier-grade NAT, multicast, ...),
/// including IPv4 addresses written inside an IPv6 one.
fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_multicast() || o[0] == 0 || o[0] >= 240 || o[0] == 100 && (64..128).contains(&o[1]))
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            if let Some(m) = v.to_ipv4_mapped().or_else(|| v.to_ipv4()) {
                return is_public_ip(IpAddr::V4(m)); // ::ffff:127.0.0.1 and ::127.0.0.1
            }
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return is_public_ip(IpAddr::V4(std::net::Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8))); // NAT64
            }
            !(v.is_loopback() || v.is_unspecified() || v.is_multicast() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 || (s[0] & 0xffc0) == 0xfec0)
        }
    }
}

/// Looks names up like the system does, then keeps only public addresses. The address that was checked is the one that is
/// connected to, so odd spellings (`127.1`, `0x7f.1`), names that point at private addresses (`127.0.0.1.nip.io`) and
/// redirects to such addresses all end here.
struct PublicOnly;

impl ureq::Resolver for PublicOnly {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
        use std::net::ToSocketAddrs;
        let all: Vec<_> = netloc.to_socket_addrs()?.collect();
        let ok: Vec<_> = all.iter().copied().filter(|a| is_public_ip(a.ip())).collect();
        if ok.is_empty() && !all.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, PRIVATE_MSG));
        }
        Ok(ok)
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout(Duration::from_secs(20)).user_agent(UA).resolver(PublicOnly).build()
}

/// An HTTP client that only connects to public addresses (the address that was checked is the one connected to) and follows no
/// redirect: for a restricted tenant's calls to a model provider.
pub(crate) fn public_agent(connect: Duration, total: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(connect).timeout(total).resolver(PublicOnly).redirects(0).build()
}

/// The address of a model server that a restricted tenant may use: https, a public host name or address, no user name or other
/// tricks in front of the host, and no spelling of a number that the system would turn into a local address.
pub(crate) fn public_https_url(url: &str) -> Result<(), String> {
    const MSG: &str = "A restricted account can only use a model server with a public https address.";
    let u = url.trim();
    if u.len() < 9 || !u[..8].eq_ignore_ascii_case("https://") || u.chars().any(|c| c.is_control() || c == ' ' || c == '\\') {
        return Err(MSG.into());
    }
    let authority = u[8..].split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err(MSG.into());
    }
    host_is_public(&format!("https://{authority}")).map_err(|_| MSG.to_string())?;
    let host = authority.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(authority).trim_matches(['[', ']']).to_lowercase();
    if !host.contains(':') && host.parse::<std::net::IpAddr>().is_err() {
        let last = host.trim_end_matches('.').rsplit('.').next().unwrap_or("");
        if last.is_empty() || last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x") {
            return Err(MSG.into());
        }
    }
    Ok(())
}

pub fn entities(s: &str) -> String {
    s.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&apos;", "'")
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    entities(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// The results on DuckDuckGo's lite page.
pub fn parse_results(html: &str) -> Vec<Hit> {
    let marks: Vec<usize> = html.match_indices("result-link").map(|(i, _)| i).collect();
    let mut hits = Vec::new();
    for (n, &i) in marks.iter().enumerate() {
        let tag_start = html[..i].rfind("<a ").unwrap_or(0);
        let Some(tag_end) = html[i..].find('>').map(|p| i + p) else { continue };
        let tag = &html[tag_start..tag_end];
        let mut url = tag.split("href=").nth(1).and_then(|h| {
            let q = h.chars().next()?;
            let body = &h[1..];
            body.find(q).map(|e| body[..e].to_string())
        }).unwrap_or_default();
        if let Some(u) = url.split("uddg=").nth(1) {
            url = percent_decode(u.split('&').next().unwrap_or(u));
        } else if url.starts_with("//") {
            url = format!("https:{url}");
        }
        let title_end = html[tag_end..].find("</a>").map(|p| tag_end + p).unwrap_or(html.len());
        let title = strip_tags(&html[tag_end + 1..title_end.max(tag_end + 1)]);
        let limit = marks.get(n + 1).copied().unwrap_or(html.len());
        let snippet = html[tag_end..limit].find("result-snippet").and_then(|p| {
            let s = &html[tag_end + p..limit];
            let a = s.find('>')? + 1;
            let b = s.find("</td>").unwrap_or(s.len());
            Some(strip_tags(&s[a..b.max(a)]))
        }).unwrap_or_default();
        if url.starts_with("http") && !title.is_empty() {
            hits.push(Hit { title, url, snippet });
        }
    }
    hits
}

pub fn search(a: &Value) -> Result<String, String> {
    let q = text(a, "query")?.trim();
    if q.is_empty() {
        return Err("The search words are empty.".into());
    }
    let resp = agent().post("https://lite.duckduckgo.com/lite/").send_form(&[("q", q)]).map_err(|e| format!("The search could not be made: {e}"))?;
    let html = resp.into_string().map_err(|e| format!("The search answer could not be read: {e}"))?;
    let hits = parse_results(&html);
    if hits.is_empty() {
        return Ok("No results (or the search engine refused this computer for now). Try other words, or open a search page in the built-in browser.".into());
    }
    Ok(hits.iter().take(8).enumerate().map(|(n, h)| format!("{}. {}\n   {}\n   {}", n + 1, h.title, h.url, h.snippet)).collect::<Vec<_>>().join("\n"))
}

fn host_is_public(url: &str) -> Result<(), String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).ok_or("Only http and https addresses can be read.")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("");
    let host = host.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(host).trim_matches(['[', ']']).to_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host.ends_with(".internal") || !host.contains('.') && !host.contains(':') {
        return Err(PRIVATE_MSG.into());
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let private = !is_public_ip(ip);
        if private {
            return Err(PRIVATE_MSG.into());
        }
    }
    Ok(())
}

/// A page's text: scripts, styles and tags are dropped.
pub fn page_text(html: &str) -> String {
    let lower_html = html.to_lowercase();
    let title = lower_html.find("<title").and_then(|a| {
        let from = html[a..].find('>')? + a + 1;
        let to = lower_html[from..].find("</title>")? + from;
        Some(strip_tags(&html[from..to]))
    });
    let mut s = html.to_string();
    for tag in ["script", "style", "noscript", "svg", "head"] {
        loop {
            let lower = s.to_lowercase();
            let Some(a) = lower.find(&format!("<{tag}")) else { break };
            let end = lower[a..].find(&format!("</{tag}>")).map(|e| a + e + tag.len() + 3).unwrap_or(s.len());
            s.replace_range(a..end.min(s.len()), " ");
        }
    }
    // line breaks at block ends keep the text readable
    for t in ["</p>", "</div>", "</li>", "</h1>", "</h2>", "</h3>", "<br>", "<br/>", "</tr>"] {
        s = s.replace(t, &format!("{t}\n"));
    }
    let body = strip_lines(&s);
    match title.filter(|t| !t.is_empty()) {
        Some(t) => format!("Title: {t}\n{body}"),
        None => body,
    }
}

fn strip_lines(s: &str) -> String {
    let mut out = Vec::new();
    for l in s.split('\n') {
        let t = strip_tags(l);
        if !t.is_empty() {
            out.push(t);
        }
    }
    out.join("\n")
}

pub fn fetch(a: &Value) -> Result<String, String> {
    let url = text(a, "url")?.trim();
    host_is_public(url)?;
    let resp = agent().get(url).set("Accept", "text/html,text/plain,application/json;q=0.9,*/*;q=0.5").call().map_err(|e| format!("The page could not be fetched: {e}"))?;
    let kind = resp.header("content-type").unwrap_or("").to_lowercase();
    if !(kind.is_empty() || kind.contains("text") || kind.contains("json") || kind.contains("xml")) {
        return Err(format!("That address is not a page of text ({kind})."));
    }
    let mut body = Vec::new();
    resp.into_reader().take(2_000_000).read_to_end(&mut body).map_err(|e| format!("The page could not be read: {e}"))?;
    let raw = String::from_utf8_lossy(&body).into_owned();
    let text = if kind.contains("html") || raw.trim_start().starts_with('<') { page_text(&raw) } else { raw };
    let max = a.get("max_chars").and_then(Value::as_u64).unwrap_or(8000).clamp(500, 20000) as usize;
    let n = text.chars().count();
    Ok(if n > max { format!("{}\n[cut: {n} characters in all; ask again with a bigger max_chars or open the page in the browser]", text.chars().take(max).collect::<String>()) } else { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_read_from_the_lite_page() {
        let html = r#"<table>
        <tr><td><a rel="nofollow" href="https://wiki.archlinux.org/title/KDE" class='result-link'>KDE - <b>ArchWiki</b></a></td></tr>
        <tr><td class='result-snippet'>To install the full set of <b>KDE</b> applications &amp; more.</td></tr>
        <tr><td><a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fkde.org%2Fannouncements%2F&rut=x" class='result-link'>Announcements</a></td></tr>
        <tr><td class='result-snippet'>All the news.</td></tr></table>"#;
        let hits = parse_results(html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "KDE - ArchWiki");
        assert_eq!(hits[0].url, "https://wiki.archlinux.org/title/KDE");
        assert_eq!(hits[0].snippet, "To install the full set of KDE applications & more.");
        assert_eq!(hits[1].url, "https://kde.org/announcements/");
        assert_eq!(hits[1].snippet, "All the news.");
    }

    #[test]
    fn only_public_addresses_are_read() {
        for bad in ["http://localhost:8080/x", "http://127.0.0.1/", "http://192.168.1.5/admin", "http://10.0.0.1", "http://[::1]/", "http://router/", "ftp://example.com/", "http://nas.local/", "http://169.254.169.254/latest", "http://user@127.0.0.1/"] {
            assert!(host_is_public(bad).is_err(), "{bad}");
        }
        for bad in ["http://[::ffff:127.0.0.1]:9/", "http://[::ffff:7f00:1]/", "http://[::ffff:a00:1]/", "http://[64:ff9b::7f00:1]/", "http://0.0.0.0/"] {
            assert!(host_is_public(bad).is_err(), "{bad}");
        }
        for good in ["https://example.com/a?b=1", "http://93.184.216.34/", "https://docs.rs:443/x"] {
            assert!(host_is_public(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn odd_spellings_of_local_addresses_are_refused_when_fetching() {
        // the system turns these into 127.0.0.1; the resolver refuses them before any connection is made
        for bad in ["http://127.1:9/", "http://0x7f.1:9/", "http://2130706433:9/", "http://[::ffff:127.0.0.1]:9/", "http://127.0.0.1.nip.io:9/", "http://localtest.me:9/"] {
            let e = fetch(&serde_json::json!({ "url": bad })).unwrap_err();
            assert!(!e.contains("Connection refused"), "{bad}: {e}");
        }
    }

    #[test]
    fn a_page_becomes_text() {
        let t = page_text("<html><head><title>x</title><style>a{}</style></head><body><h1>Hello</h1><script>var a=1;</script><p>One &amp; two</p><div>Three</div></body></html>");
        assert_eq!(t, "Title: x\nHello\nOne & two\nThree");
    }

    /// Needs the internet: `cargo test -p zo-core live_web -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_web_search_and_fetch() {
        let r = search(&serde_json::json!({"query": "KDE Plasma latest release"})).unwrap();
        println!("{r}");
        assert!(r.starts_with("1. "), "no results: {r}");
        let page = fetch(&serde_json::json!({"url": "https://example.com", "max_chars": 500})).unwrap();
        println!("{page}");
        assert!(page.contains("Example Domain"));
        assert!(fetch(&serde_json::json!({"url": "http://127.0.0.1:1/"})).is_err());
    }
}
