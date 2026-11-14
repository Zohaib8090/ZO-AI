//! Update checks (docs/UPDATES.md). The source of truth is the newest GitHub Release of the repository (`ZO_UPDATE_REPO`, default
//! `Zohaib8090/zohara-agent`); a release carries a `latest.json` asset:
//!
//! ```json
//! {"version": "0.2.0", "date": "2026-11-20", "notes": "...", "min_supported": "0.1.0",
//!  "assets": {"apk": "zohara-agent-0.2.0.apk", "pkg": "zohara-agent-0.2.0-x86_64.pkg.tar.zst", "aab": "zohara-agent-0.2.0.aab",
//!             "sha256s": {"zohara-agent-0.2.0.apk": "<64 hex>"}}}
//! ```
//!
//! Three parts:
//!
//! * **The pure part**: [`Semver`] and its order, [`Latest::parse`], [`Cache`] (one hour, a failed fetch is not repeated for five minutes and
//!   the last good answer is served as `stale`), [`api_body`] (the JSON the server sends) and [`Status::from_api`] (what a client makes of it).
//! * **The server side**: [`fetch_latest`] reads GitHub. Only the SERVER does that, so no client address and no user data reaches GitHub.
//! * **The client side**: [`check_server`] asks `GET /api/update/latest` of the Zohara Agent server. Nothing is installed by any of this;
//!   the clients only tell the person and open the release page.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

/// The version of this build: the Cargo workspace version, the single source (the server reports it as `server_version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_REPO: &str = "Zohaib8090/zohara-agent";
/// How long a good answer is served without asking GitHub again.
pub const CACHE_SECS: u64 = 3600;
/// After a failed fetch (unreachable, rate limited) GitHub is left alone this long.
pub const RETRY_SECS: u64 = 300;
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BODY: u64 = 256 * 1024;
const MAX_NOTES: usize = 4000;

// ---------------------------------------------------------------------------------------------------------------------------
// versions

/// A version: up to four numbers (`0.1.0` and the package's `0.1.0.202611141200`) and an optional pre-release (`-beta.1`).
/// A leading `v` and build metadata (`+abc`) are ignored. A pre-release is older than the same version without one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Semver {
    nums: [u64; 4],
    pre: Vec<Ident>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ident {
    // numbers come before words (derive order)
    Num(u64),
    Word(String),
}

impl Semver {
    pub fn parse(s: &str) -> Option<Semver> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let s = s.split('+').next().unwrap_or("");
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let parts: Vec<&str> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 4 {
            return None;
        }
        let mut nums = [0u64; 4];
        for (i, p) in parts.iter().enumerate() {
            if p.is_empty() || p.len() > 12 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            nums[i] = p.parse().ok()?;
        }
        let mut idents = Vec::new();
        if let Some(pre) = pre {
            for id in pre.split('.') {
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return None;
                }
                idents.push(match id.parse::<u64>() {
                    Ok(n) if id.bytes().all(|b| b.is_ascii_digit()) => Ident::Num(n),
                    _ => Ident::Word(id.to_string()),
                });
            }
        }
        Some(Semver { nums, pre: idents })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl PartialOrd for Semver {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Semver {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match self.nums.cmp(&other.nums) {
            Equal => {}
            o => return o,
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Equal,
            (true, false) => Greater,
            (false, true) => Less,
            // the identifiers one by one; when all of the shorter list match, the longer one is newer
            (false, false) => self.pre.cmp(&other.pre),
        }
    }
}

/// Is `latest` newer than `current`? A version that cannot be read is never "newer" (no false alarms).
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (Semver::parse(current), Semver::parse(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// latest.json

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assets {
    /// File names (not addresses) of the files attached to the release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aab: Option<String>,
    /// file name -> sha256 (64 hex characters)
    #[serde(default)]
    pub sha256s: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Latest {
    pub version: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub notes: String,
    /// Versions older than this are no longer supported: the clients say an update is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_supported: Option<String>,
    #[serde(default)]
    pub assets: Assets,
    /// The page of the release (always `https://github.com/<repo>/releases/tag/v<version>`).
    #[serde(default)]
    pub release_url: String,
}

fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(max).collect::<String>().trim().to_string()
}

fn clean_name(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    (!s.is_empty() && s.len() <= 160 && !s.starts_with('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))).then(|| s.to_string())
}

/// `owner/name`, letters, digits and `_ . -` only.
pub fn valid_repo(r: &str) -> bool {
    let Some((o, n)) = r.split_once('/') else { return false };
    let ok = |p: &str| !p.is_empty() && p.len() <= 100 && !p.starts_with('.') && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    ok(o) && ok(n)
}

/// The repository to read: `ZO_UPDATE_REPO` when it is valid, else the default.
pub fn repo() -> String {
    repo_of(std::env::var("ZO_UPDATE_REPO").ok().as_deref())
}

pub fn repo_of(env: Option<&str>) -> String {
    match env.map(str::trim) {
        Some(r) if valid_repo(r) => r.to_string(),
        _ => DEFAULT_REPO.to_string(),
    }
}

pub fn release_url(repo: &str, version: &str) -> String {
    format!("https://github.com/{repo}/releases/tag/v{}", version.trim_start_matches(['v', 'V']))
}

impl Latest {
    /// Reads a `latest.json`. A version that is not a version is an error; broken optional parts are left out, never trusted.
    pub fn parse(v: &Value, repo: &str) -> Result<Latest, String> {
        let version = v.get("version").and_then(Value::as_str).unwrap_or("").trim().trim_start_matches(['v', 'V']).to_string();
        if Semver::parse(&version).is_none() {
            return Err("the release has no readable version".into());
        }
        let min = v.get("min_supported").and_then(Value::as_str).map(str::trim).filter(|m| Semver::parse(m).is_some()).map(|m| m.trim_start_matches(['v', 'V']).to_string());
        let a = v.get("assets");
        let mut assets = Assets { apk: clean_name(a.and_then(|a| a.get("apk"))), pkg: clean_name(a.and_then(|a| a.get("pkg"))), aab: clean_name(a.and_then(|a| a.get("aab"))), sha256s: BTreeMap::new() };
        if let Some(m) = a.and_then(|a| a.get("sha256s")).and_then(Value::as_object) {
            for (k, h) in m.iter().take(20) {
                let h = h.as_str().unwrap_or("").to_ascii_lowercase();
                if clean_name(Some(&json!(k))).is_some() && h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    assets.sha256s.insert(k.clone(), h);
                }
            }
        }
        Ok(Latest {
            release_url: release_url(repo, &version),
            version,
            date: clean_text(v.get("date").and_then(Value::as_str).unwrap_or(""), 40),
            notes: clean_text(v.get("notes").and_then(Value::as_str).unwrap_or(""), MAX_NOTES),
            min_supported: min,
            assets,
        })
    }

    /// A release without a `latest.json`: what GitHub's own release record says (tag, date, text).
    pub fn from_release_record(v: &Value, repo: &str) -> Result<Latest, String> {
        let tag = v.get("tag_name").and_then(Value::as_str).unwrap_or("");
        Latest::parse(&json!({"version": tag, "date": v.get("published_at").and_then(Value::as_str).unwrap_or("").get(..10).unwrap_or(""), "notes": v.get("body").and_then(Value::as_str).unwrap_or("")}), repo)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// the server side: reading GitHub

/// Where the releases are read from. `web` serves `/<repo>/releases/latest/download/latest.json`, `api` serves `/repos/<repo>/releases/latest`.
#[derive(Clone, Debug)]
pub struct Source {
    pub repo: String,
    pub web: String,
    pub api: String,
}

impl Source {
    pub fn github(repo: &str) -> Source {
        Source { repo: repo.into(), web: "https://github.com".into(), api: "https://api.github.com".into() }
    }

    /// For tests and mirrors: both come from one address (`<base>` and `<base>/api`).
    pub fn at(repo: &str, base: &str) -> Source {
        let b = base.trim_end_matches('/');
        Source { repo: repo.into(), web: b.into(), api: format!("{b}/api") }
    }

    /// The configured source: `ZO_UPDATE_REPO`, and `ZO_UPDATE_BASE_URL` (a mirror; for tests) when set.
    pub fn from_env() -> Source {
        let repo = repo();
        match std::env::var("ZO_UPDATE_BASE_URL").ok().map(|b| b.trim().to_string()).filter(|b| b.starts_with("http://") || b.starts_with("https://")) {
            Some(b) => Source::at(&repo, &b),
            None => Source::github(&repo),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(FETCH_TIMEOUT).redirects(5).user_agent(&format!("zohara-agent-update-check/{VERSION}")).build()
}

fn get_json(url: &str) -> Result<Value, (u16, String)> {
    match agent().get(url).set("Accept", "application/json").call() {
        Ok(r) => {
            let mut body = String::new();
            r.into_reader().take(MAX_BODY).read_to_string(&mut body).map_err(|_| (0, "GitHub's answer could not be read".to_string()))?;
            serde_json::from_str(&body).map_err(|_| (0, "GitHub's answer is not JSON".to_string()))
        }
        Err(ureq::Error::Status(code, _)) => Err((code, format!("GitHub answered {code}"))),
        Err(_) => Err((0, "GitHub could not be reached".to_string())),
    }
}

/// Reads the newest release: its `latest.json`, else GitHub's release record. Errors are short sentences for the logs and the clients.
pub fn fetch_latest(src: &Source) -> Result<Latest, String> {
    match get_json(&format!("{}/{}/releases/latest/download/latest.json", src.web, src.repo)) {
        Ok(v) => return Latest::parse(&v, &src.repo),
        Err((404, _)) => {}
        Err((403 | 429, _)) => return Err("GitHub is limiting requests right now".into()),
        Err((_, e)) => return Err(e),
    }
    // no latest.json on the newest release (or no release at all): the release record
    match get_json(&format!("{}/repos/{}/releases/latest", src.api, src.repo)) {
        Ok(v) => Latest::from_release_record(&v, &src.repo),
        Err((404, _)) => Err("no release has been published yet".into()),
        Err((403 | 429, _)) => Err("GitHub is limiting requests right now".into()),
        Err((_, e)) => Err(e),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// the cache

#[derive(Default)]
struct State {
    good: Option<(Latest, u64)>,
    failed: Option<(u64, String)>,
}

/// What the server knows right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub latest: Option<Latest>,
    /// When `latest` was read (seconds since 1970).
    pub checked_at: Option<u64>,
    /// `latest` is older than the cache time because the last try failed.
    pub stale: bool,
    pub error: Option<String>,
}

/// One hour of memory for the newest release. `get` asks `fetch` at most once per hour; after a failure it does not ask again for five
/// minutes and answers with the last good value (marked `stale`). The lock is held during the fetch, so a crowd of callers makes one request.
#[derive(Default)]
pub struct Cache {
    state: Mutex<State>,
}

impl Cache {
    pub const fn new() -> Cache {
        Cache { state: Mutex::new(State { good: None, failed: None }) }
    }

    pub fn get(&self, now: u64, fetch: impl FnOnce() -> Result<Latest, String>) -> Snapshot {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((l, at)) = &s.good {
            if now.saturating_sub(*at) < CACHE_SECS {
                return Snapshot { latest: Some(l.clone()), checked_at: Some(*at), stale: false, error: None };
            }
        }
        if let Some((at, e)) = &s.failed {
            if now.saturating_sub(*at) < RETRY_SECS {
                let (latest, checked_at) = s.good.as_ref().map(|(l, a)| (Some(l.clone()), Some(*a))).unwrap_or((None, None));
                return Snapshot { stale: latest.is_some(), latest, checked_at, error: Some(e.clone()) };
            }
        }
        match fetch() {
            Ok(l) => {
                s.good = Some((l.clone(), now));
                s.failed = None;
                Snapshot { latest: Some(l), checked_at: Some(now), stale: false, error: None }
            }
            Err(e) => {
                s.failed = Some((now, e.clone()));
                let (latest, checked_at) = s.good.as_ref().map(|(l, a)| (Some(l.clone()), Some(*a))).unwrap_or((None, None));
                Snapshot { stale: latest.is_some(), latest, checked_at, error: Some(e) }
            }
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The body of `GET /api/update/latest`. Always `ok: true`: "could not find out" is a normal answer (`latest: null` and `error`), so a
/// client shows "could not check" and not a failure of the server. It holds nothing about the caller.
pub fn api_body(s: &Snapshot, repo: &str) -> Value {
    let mut v = json!({
        "ok": true,
        "server_version": VERSION,
        "repo": repo,
        "latest": s.latest,
        "checked_at": s.checked_at,
        "stale": s.stale,
    });
    if let Some(e) = &s.error {
        v["error"] = json!(e);
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------------------
// the client side

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    UpToDate,
    Available,
    /// The server could not find out (no release yet, GitHub unreachable, an answer that is not understood).
    Unknown,
}

/// What a client concludes from the server's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub current: String,
    pub server_version: Option<String>,
    pub latest: Option<Latest>,
    pub verdict: Verdict,
    /// This version is older than `min_supported`: the update is not optional any more.
    pub required: bool,
    pub stale: bool,
    pub error: Option<String>,
}

impl Status {
    pub fn from_api(current: &str, v: &Value) -> Status {
        let server_version = v.get("server_version").and_then(Value::as_str).map(str::to_string);
        let latest = v.get("latest").filter(|l| l.is_object()).and_then(|l| {
            // the server already cleaned it; the repo only matters for the release address, which we keep from the answer
            let repo = v.get("repo").and_then(Value::as_str).filter(|r| valid_repo(r)).unwrap_or(DEFAULT_REPO);
            Latest::parse(l, repo).ok()
        });
        let error = v.get("error").and_then(Value::as_str).map(|e| clean_text(e, 200)).filter(|e| !e.is_empty());
        let verdict = match &latest {
            Some(l) if is_newer(current, &l.version) => Verdict::Available,
            Some(_) => Verdict::UpToDate,
            None => Verdict::Unknown,
        };
        let required = latest.as_ref().and_then(|l| l.min_supported.as_deref()).is_some_and(|m| is_newer(current, m));
        Status { current: current.into(), server_version, latest, verdict, required, stale: v.get("stale").and_then(Value::as_bool) == Some(true), error }
    }

    /// One line for a terminal or a label.
    pub fn summary(&self) -> String {
        match (&self.verdict, &self.latest) {
            (Verdict::Available, Some(l)) => format!("Version {} is available (you have {}).", l.version, self.current),
            (Verdict::UpToDate, Some(_)) => format!("You are up to date ({}).", self.current),
            _ => match &self.error {
                Some(e) => format!("Could not check for updates: {e}."),
                None => "Could not check for updates.".to_string(),
            },
        }
    }
}

/// Asks the Zohara Agent server (never GitHub) for the newest release and compares it with `current`.
pub fn check_server(base: &str, current: &str) -> Result<Status, String> {
    let url = format!("{}/api/update/latest", base.trim_end_matches('/'));
    let v = agent_short().get(&url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("the server answered {code}"),
        _ => "the server could not be reached".to_string(),
    })?;
    let mut body = String::new();
    v.into_reader().take(MAX_BODY).read_to_string(&mut body).map_err(|_| "could not read the server's answer".to_string())?;
    let v: Value = serde_json::from_str(&body).map_err(|_| "the server's answer is not JSON".to_string())?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("the server does not support update checks".into());
    }
    Ok(Status::from_api(current, &v))
}

fn agent_short() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(FETCH_TIMEOUT).redirects(3).user_agent(&format!("zohara-agent/{VERSION}")).build()
}

// ---------------------------------------------------------------------------------------------------------------------------
// the desktop's memory: which version was announced already, when the last check ran, whether to check by itself

/// How often the desktop app asks while it runs.
pub const CHECK_EVERY_SECS: u64 = 12 * 3600;

/// `<config>/update-state.json`.
pub fn state_path() -> std::path::PathBuf {
    crate::audit::config_dir().join("update-state.json")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopState {
    /// The newest version the person was already told about.
    pub last_notified: Option<String>,
    /// When the last automatic check ran (seconds since 1970); 0 = never.
    pub last_check: u64,
    /// "Check for updates automatically" (default on).
    pub auto: bool,
}

pub fn read_state(path: &std::path::Path) -> DesktopState {
    let v: Value = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    DesktopState {
        last_notified: v.get("last_notified").and_then(Value::as_str).filter(|s| Semver::parse(s).is_some()).map(str::to_string),
        last_check: v.get("last_check").and_then(Value::as_u64).unwrap_or(0),
        auto: v.get("auto").and_then(Value::as_bool).unwrap_or(true),
    }
}

pub fn write_state(path: &std::path::Path, st: &DesktopState) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let body = json!({"last_notified": st.last_notified, "last_check": st.last_check, "auto": st.auto}).to_string();
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn change(path: &std::path::Path, f: impl FnOnce(&mut DesktopState)) {
    let mut st = read_state(path);
    f(&mut st);
    write_state(path, &st);
}

pub fn last_notified() -> Option<String> {
    read_state(&state_path()).last_notified
}

pub fn remember_notified(version: &str) {
    change(&state_path(), |s| s.last_notified = Some(version.trim_start_matches(['v', 'V']).to_string()));
}

pub fn auto_check_enabled() -> bool {
    read_state(&state_path()).auto
}

pub fn set_auto_check(on: bool) {
    change(&state_path(), |s| s.auto = on);
}

pub fn remember_checked(now: u64) {
    change(&state_path(), |s| s.last_check = now);
}

/// Is an automatic check due? Only when the switch is on and the last one was at least [`CHECK_EVERY_SECS`] ago (a clock that went
/// backwards counts as due, so a wrong date cannot stop the checks for good).
pub fn check_due(st: &DesktopState, now: u64) -> bool {
    st.auto && (st.last_check > now || now - st.last_check >= CHECK_EVERY_SECS)
}

/// The command that installs the Arch package of this release once it is downloaded to the current folder, when the release has one.
/// (The file name was checked against a strict character set when `latest.json` was read, so it is safe in a shell.)
pub fn pacman_command(l: &Latest) -> Option<String> {
    l.assets.pkg.as_ref().map(|p| format!("sudo pacman -U {p}"))
}

/// Tell the person about this status? Only when a newer version is available and it is not the one they were told about last time
/// (or an even newer one has come out since).
pub fn should_notify(status: &Status, last: Option<&str>) -> bool {
    let (Verdict::Available, Some(l)) = (&status.verdict, &status.latest) else { return false };
    match last {
        Some(prev) => is_newer(prev, &l.version),
        None => true,
    }
}

// ---------------------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn v(s: &str) -> Semver {
        Semver::parse(s).unwrap_or_else(|| panic!("{s}"))
    }

    #[test]
    fn semver_orders_numbers_numerically() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("1.2.10") > v("1.2.9"));
        assert!(v("0.1.0") == v("v0.1.0"));
        assert!(v("0.1") == v("0.1.0"));
        assert!(v("1") == v("1.0.0"));
        // the package version has a fourth number (a build time)
        assert!(v("0.1.0.202611141300") > v("0.1.0.202611141200"));
        assert!(v("0.1.1") > v("0.1.0.202699999999"));
    }

    #[test]
    fn semver_prerelease_rules_follow_the_spec() {
        let order = ["1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0"];
        for w in order.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
        }
        assert!(v("1.0.0-rc.1").is_prerelease());
        assert!(!v("1.0.0").is_prerelease());
        // build metadata does not count
        assert!(v("1.0.0+abc") == v("1.0.0+xyz"));
        assert!(v("1.0.0+abc") == v("1.0.0"));
    }

    #[test]
    fn semver_rejects_nonsense() {
        for bad in ["", "v", "a.b.c", "1.2.3.4.5", "1..2", "1.2.x", "-1.0.0", "1.0.0-", "1.0.0-a..b", "99999999999999999999.0.0", "1.0.0 beta", "latest"] {
            assert!(Semver::parse(bad).is_none(), "{bad:?} must not parse");
        }
        assert!(Semver::parse(" v1.2.3 ").is_some());
    }

    #[test]
    fn is_newer_never_raises_a_false_alarm() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.3.0", "0.2.0"));
        assert!(!is_newer("0.1.0", "garbage"));
        assert!(!is_newer("garbage", "9.9.9"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0-rc.1"));
    }

    fn sample() -> Value {
        json!({"version": "v0.2.0", "date": "2026-11-20", "notes": "Update checks.\nBug fixes.", "min_supported": "0.1.0",
               "assets": {"apk": "zohara-agent-0.2.0.apk", "pkg": "zohara-agent-0.2.0-x86_64.pkg.tar.zst", "aab": "zohara-agent-0.2.0.aab",
                          "sha256s": {"zohara-agent-0.2.0.apk": "A".repeat(64), "bad": "xyz", "../evil": "b".repeat(64)}}})
    }

    #[test]
    fn latest_json_is_read_and_cleaned() {
        let l = Latest::parse(&sample(), "Zohaib8090/zohara-agent").unwrap();
        assert_eq!(l.version, "0.2.0");
        assert_eq!(l.date, "2026-11-20");
        assert_eq!(l.min_supported.as_deref(), Some("0.1.0"));
        assert_eq!(l.assets.apk.as_deref(), Some("zohara-agent-0.2.0.apk"));
        assert_eq!(l.release_url, "https://github.com/Zohaib8090/zohara-agent/releases/tag/v0.2.0");
        // only the one good checksum survives, lower-cased
        assert_eq!(l.assets.sha256s.len(), 1);
        assert_eq!(l.assets.sha256s["zohara-agent-0.2.0.apk"], "a".repeat(64));
    }

    #[test]
    fn latest_json_needs_a_version_and_drops_unsafe_parts() {
        assert!(Latest::parse(&json!({"notes": "x"}), "a/b").is_err());
        assert!(Latest::parse(&json!({"version": "soon"}), "a/b").is_err());
        let l = Latest::parse(&json!({"version": "1.0.0", "min_supported": "nope", "assets": {"apk": "../../etc/passwd", "pkg": "a b.pkg", "aab": ".hidden"}, "notes": "a\u{7}b"}), "a/b").unwrap();
        assert_eq!(l.min_supported, None);
        assert_eq!(l.assets, Assets::default());
        assert_eq!(l.notes, "ab");
        let long = "x".repeat(10_000);
        assert_eq!(Latest::parse(&json!({"version": "1.0.0", "notes": long}), "a/b").unwrap().notes.len(), MAX_NOTES);
    }

    #[test]
    fn release_record_is_the_fallback_without_latest_json() {
        let rec = json!({"tag_name": "v0.3.1", "published_at": "2026-12-01T10:00:00Z", "body": "What is new"});
        let l = Latest::from_release_record(&rec, "o/r").unwrap();
        assert_eq!((l.version.as_str(), l.date.as_str(), l.notes.as_str()), ("0.3.1", "2026-12-01", "What is new"));
        assert!(Latest::from_release_record(&json!({"tag_name": "nightly"}), "o/r").is_err());
    }

    #[test]
    fn repo_comes_from_the_environment_only_when_valid() {
        assert_eq!(repo_of(None), DEFAULT_REPO);
        assert_eq!(repo_of(Some("me/my-fork")), "me/my-fork");
        assert_eq!(repo_of(Some(" me/my.fork_2 ")), "me/my.fork_2");
        for bad in ["", "noslash", "a/b/c", "a b/c", "../x", "a/..%2f", "/b", "a/"] {
            assert_eq!(repo_of(Some(bad)), DEFAULT_REPO, "{bad:?}");
        }
    }

    fn rel(version: &str) -> Latest {
        Latest::parse(&json!({"version": version}), "o/r").unwrap()
    }

    #[test]
    fn cache_asks_once_per_hour() {
        let c = Cache::new();
        let calls = AtomicUsize::new(0);
        let fetch = || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(rel("0.2.0"))
        };
        let a = c.get(1000, fetch);
        assert_eq!((a.latest.as_ref().map(|l| l.version.as_str()), a.checked_at, a.stale), (Some("0.2.0"), Some(1000), false));
        c.get(1000 + CACHE_SECS - 1, fetch);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "inside the hour nothing is fetched");
        c.get(1000 + CACHE_SECS, fetch);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "after the hour it is fetched again");
    }

    #[test]
    fn cache_falls_back_to_the_last_good_answer_and_backs_off() {
        let c = Cache::new();
        c.get(0, || Ok(rel("0.2.0")));
        let calls = AtomicUsize::new(0);
        let fail = || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("GitHub is limiting requests right now".to_string())
        };
        let s = c.get(CACHE_SECS + 10, fail);
        assert_eq!(s.latest.as_ref().map(|l| l.version.as_str()), Some("0.2.0"));
        assert!(s.stale);
        assert_eq!(s.checked_at, Some(0));
        assert_eq!(s.error.as_deref(), Some("GitHub is limiting requests right now"));
        // inside the back-off nothing is asked, the stale answer is repeated
        let s2 = c.get(CACHE_SECS + 10 + RETRY_SECS - 1, fail);
        assert!(s2.stale);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // after it, one more try, which may succeed
        let s3 = c.get(CACHE_SECS + 10 + RETRY_SECS, || Ok(rel("0.3.0")));
        assert_eq!((s3.latest.map(|l| l.version), s3.stale, s3.error), (Some("0.3.0".to_string()), false, None));
    }

    #[test]
    fn cache_with_nothing_good_answers_null_and_does_not_hammer() {
        let c = Cache::new();
        let calls = AtomicUsize::new(0);
        let fail = || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("no release has been published yet".to_string())
        };
        let s = c.get(50, fail);
        assert!(s.latest.is_none() && !s.stale);
        c.get(60, fail);
        c.get(50 + RETRY_SECS - 1, fail);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let body = api_body(&s, "o/r");
        assert_eq!(body["ok"], true);
        assert!(body["latest"].is_null());
        assert_eq!(body["error"], "no release has been published yet");
        assert_eq!(body["server_version"], VERSION);
        assert_eq!(body["repo"], "o/r");
    }

    #[test]
    fn api_body_carries_no_user_data_and_round_trips_to_a_status() {
        let s = Snapshot { latest: Some(Latest::parse(&sample(), "o/r").unwrap()), checked_at: Some(7), stale: false, error: None };
        let body = api_body(&s, "o/r");
        let keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["checked_at", "latest", "ok", "repo", "server_version", "stale"]);
        let st = Status::from_api("0.1.0", &body);
        assert_eq!(st.verdict, Verdict::Available);
        assert!(!st.required);
        assert_eq!(st.latest.as_ref().unwrap().version, "0.2.0");
        assert_eq!(st.latest.as_ref().unwrap().release_url, "https://github.com/o/r/releases/tag/v0.2.0");
        assert_eq!(st.summary(), "Version 0.2.0 is available (you have 0.1.0).");
        assert_eq!(Status::from_api("0.2.0", &body).verdict, Verdict::UpToDate);
        assert_eq!(Status::from_api("0.9.0", &body).verdict, Verdict::UpToDate);
    }

    #[test]
    fn status_marks_unsupported_versions_and_unknown_answers() {
        let body = api_body(&Snapshot { latest: Some(Latest::parse(&json!({"version": "2.0.0", "min_supported": "1.5.0"}), "o/r").unwrap()), checked_at: Some(1), stale: true, error: Some("x".into()) }, "o/r");
        let old = Status::from_api("1.0.0", &body);
        assert!(old.required && old.verdict == Verdict::Available && old.stale);
        assert!(!Status::from_api("1.5.0", &body).required);
        let none = Status::from_api("1.0.0", &json!({"ok": true, "latest": null, "error": "GitHub could not be reached"}));
        assert_eq!(none.verdict, Verdict::Unknown);
        assert_eq!(none.summary(), "Could not check for updates: GitHub could not be reached.");
        assert_eq!(Status::from_api("1.0.0", &json!({"latest": {"version": "bad"}})).verdict, Verdict::Unknown);
        assert_eq!(Status::from_api("1.0.0", &json!(null)).verdict, Verdict::Unknown);
    }

    #[test]
    fn the_pacman_command_uses_the_package_file_name() {
        let l = Latest::parse(&sample(), "o/r").unwrap();
        assert_eq!(pacman_command(&l).as_deref(), Some("sudo pacman -U zohara-agent-0.2.0-x86_64.pkg.tar.zst"));
        assert_eq!(pacman_command(&rel("1.0.0")), None);
    }

    #[test]
    fn notify_once_per_version() {
        let body = api_body(&Snapshot { latest: Some(rel("0.2.0")), checked_at: Some(1), stale: false, error: None }, "o/r");
        let st = Status::from_api("0.1.0", &body);
        assert!(should_notify(&st, None));
        assert!(!should_notify(&st, Some("0.2.0")));
        assert!(!should_notify(&st, Some("v0.2.0")));
        assert!(should_notify(&st, Some("0.1.5")));
        assert!(!should_notify(&Status::from_api("0.2.0", &body), None), "up to date: nothing to say");
        assert!(!should_notify(&Status::from_api("0.1.0", &json!({"latest": null})), None));
    }

    #[test]
    fn desktop_state_is_remembered_in_a_file_and_survives_each_change() {
        let dir = std::env::temp_dir().join(format!("zo-upd-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = dir.join("sub/update-state.json");
        assert_eq!(read_state(&f), DesktopState { last_notified: None, last_check: 0, auto: true });
        let mut st = read_state(&f);
        st.last_notified = Some("0.2.0".into());
        st.last_check = 1234;
        st.auto = false;
        write_state(&f, &st);
        assert_eq!(read_state(&f), st);
        std::fs::write(&f, "{\"last_notified\": \"junk\"}").unwrap();
        assert_eq!(read_state(&f), DesktopState { last_notified: None, last_check: 0, auto: true });
        std::fs::write(&f, "not json").unwrap();
        assert!(read_state(&f).auto);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn automatic_checks_are_due_every_twelve_hours_and_only_when_switched_on() {
        let st = |last, auto| DesktopState { last_notified: None, last_check: last, auto };
        assert!(check_due(&st(0, true), 1_000_000));
        assert!(!check_due(&st(1_000_000, true), 1_000_000 + CHECK_EVERY_SECS - 1));
        assert!(check_due(&st(1_000_000, true), 1_000_000 + CHECK_EVERY_SECS));
        assert!(!check_due(&st(0, false), 1_000_000), "switched off: never");
        assert!(check_due(&st(2_000_000, true), 1_000_000), "a clock that went back must not stop the checks");
    }

    /// The file the release workflow makes (packaging/make-latest-json.sh) must be one this parser accepts. Skipped where jq is missing.
    #[test]
    fn the_release_script_makes_a_latest_json_the_clients_accept() {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/make-latest-json.sh");
        if std::process::Command::new("jq").arg("--version").output().is_err() || !script.is_file() {
            eprintln!("skipped: jq or the script is not here");
            return;
        }
        let dir = std::env::temp_dir().join(format!("zo-latest-json-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("zohara-agent-0.2.0.apk"), "apk").unwrap();
        std::fs::write(dir.join("zohara-agent-0.2.0.aab"), "aab").unwrap();
        std::fs::write(dir.join("zohara-agent-0.2.0-1-x86_64.pkg.tar.zst"), "pkg").unwrap();
        let notes = dir.join("notes.md");
        std::fs::write(&notes, "Update checks \"quoted\"\n- more").unwrap();
        let out = std::process::Command::new("bash").arg(&script).args(["v0.2.0"]).arg(&dir).arg(&notes).env("MIN_SUPPORTED", "0.1.0").env("RELEASE_DATE", "2026-11-20").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        let l = Latest::parse(&v, "o/r").unwrap();
        assert_eq!((l.version.as_str(), l.date.as_str(), l.min_supported.as_deref()), ("0.2.0", "2026-11-20", Some("0.1.0")));
        assert_eq!(l.notes, "Update checks \"quoted\"\n- more");
        assert_eq!(l.assets.apk.as_deref(), Some("zohara-agent-0.2.0.apk"));
        assert_eq!(l.assets.aab.as_deref(), Some("zohara-agent-0.2.0.aab"));
        assert_eq!(l.assets.pkg.as_deref(), Some("zohara-agent-0.2.0-1-x86_64.pkg.tar.zst"));
        // every listed file has its checksum (sha256 of "apk")
        assert_eq!(l.assets.sha256s.len(), 3);
        assert_eq!(l.assets.sha256s["zohara-agent-0.2.0.apk"], sha256_hex(b"apk"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sha256_hex(b: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    /// A tiny HTTP server: answers each request by path with (status, body); counts the requests.
    fn mock(routes: Vec<(&'static str, u16, String)>) -> (String, std::sync::Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                h.fetch_add(1, Ordering::SeqCst);
                let (code, body) = routes.iter().find(|(p, _, _)| *p == path).map(|(_, c, b)| (*c, b.clone())).unwrap_or((404, "{}".into()));
                let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        (base, hits)
    }

    #[test]
    fn fetch_reads_latest_json_from_the_release() {
        let (base, _) = mock(vec![("/o/r/releases/latest/download/latest.json", 200, sample().to_string())]);
        let l = fetch_latest(&Source::at("o/r", &base)).unwrap();
        assert_eq!(l.version, "0.2.0");
    }

    #[test]
    fn fetch_falls_back_to_the_release_record_and_reports_no_release() {
        let rec = json!({"tag_name": "v0.4.0", "published_at": "2026-12-02T00:00:00Z", "body": "notes"}).to_string();
        let (base, _) = mock(vec![("/api/repos/o/r/releases/latest", 200, rec)]);
        assert_eq!(fetch_latest(&Source::at("o/r", &base)).unwrap().version, "0.4.0");
        let (empty, _) = mock(vec![]);
        assert_eq!(fetch_latest(&Source::at("o/r", &empty)).unwrap_err(), "no release has been published yet");
    }

    #[test]
    fn fetch_failures_are_short_sentences() {
        let (limited, _) = mock(vec![("/o/r/releases/latest/download/latest.json", 429, "{}".into())]);
        assert_eq!(fetch_latest(&Source::at("o/r", &limited)).unwrap_err(), "GitHub is limiting requests right now");
        let (broken, _) = mock(vec![("/o/r/releases/latest/download/latest.json", 500, "{}".into())]);
        assert_eq!(fetch_latest(&Source::at("o/r", &broken)).unwrap_err(), "GitHub answered 500");
        let (junk, _) = mock(vec![("/o/r/releases/latest/download/latest.json", 200, "<html>".into())]);
        assert_eq!(fetch_latest(&Source::at("o/r", &junk)).unwrap_err(), "GitHub's answer is not JSON");
        // nothing listens on port 9
        assert_eq!(fetch_latest(&Source::at("o/r", "http://127.0.0.1:9")).unwrap_err(), "GitHub could not be reached");
    }

    #[test]
    fn client_check_reads_the_server_answer() {
        let body = api_body(&Snapshot { latest: Some(rel("0.2.0")), checked_at: Some(5), stale: false, error: None }, "o/r").to_string();
        let (base, _) = mock(vec![("/api/update/latest", 200, body)]);
        let st = check_server(&base, "0.1.0").unwrap();
        assert_eq!(st.verdict, Verdict::Available);
        let (old, _) = mock(vec![]);
        assert_eq!(check_server(&old, "0.1.0").unwrap_err(), "the server answered 404");
        assert!(check_server("http://127.0.0.1:9", "0.1.0").unwrap_err().contains("could not be reached"));
        let (bad, _) = mock(vec![("/api/update/latest", 200, "{\"ok\": false}".into())]);
        assert!(check_server(&bad, "0.1.0").is_err());
    }
}
