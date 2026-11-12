//! What is running right now: answers being written, Claude Code runs, scheduled tasks, the memory agent, long commands, terminals,
//! the sync and the clients (phone, web, command line) that are connected. One registry for the whole process, so the desktop's tray
//! and Activity page, the phone and the web app all show (and can stop) the same things.
//!
//! Things that start and end register with `start`, which gives back a `Guard`: the item goes away when the guard is dropped, on
//! every way out of the code (early return, error, panic, Stop). Terminals and the sync are not registered; they are read from
//! where they live when `list` is called, so they can never be out of date.
//!
//! The registry knows tenants. On a multi-tenant server a tenant sees and stops only its own items; a restricted tenant only has
//! answers, memory jobs and clients. Without tenants (the desktop) everything is visible.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Answer,
    ClaudeCode,
    Terminal,
    Scheduled,
    MemoryJob,
    Sync,
    Client,
    Command,
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Answer => "answer",
            Kind::ClaudeCode => "claude_code",
            Kind::Terminal => "terminal",
            Kind::Scheduled => "scheduled",
            Kind::MemoryJob => "memory_job",
            Kind::Sync => "sync",
            Kind::Client => "client",
            Kind::Command => "command",
            Kind::Other => "other",
        }
    }

    /// The kinds a restricted tenant can have.
    fn for_restricted(self) -> bool {
        matches!(self, Kind::Answer | Kind::MemoryJob | Kind::Client)
    }
}

#[derive(Clone)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    /// "desktop", "phone", "web", "cli", "scheduler" or "background".
    pub source: String,
    pub chat_id: Option<String>,
    pub started_ms: u64,
    /// Setting this stops the thing. None: it can only be watched.
    pub cancel: Option<Arc<AtomicBool>>,
    pub tenant: Option<String>,
    /// Closing a terminal is the stop of a terminal.
    term: Option<u32>,
}

impl Item {
    pub fn can_stop(&self) -> bool {
        self.cancel.is_some() || self.term.is_some()
    }

    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "kind": self.kind.as_str(), "title": self.title, "detail": self.detail, "source": self.source,
            "chat_id": self.chat_id, "started_ms": self.started_ms, "can_stop": self.can_stop()})
    }
}

static ITEMS: Mutex<Vec<Item>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
/// Ids from here up are terminals (this + the terminal's number) and the sync; they are made when asked, not stored.
const TERM_BASE: u64 = 1 << 40;
const SYNC_ID: u64 = (1 << 41) + 1;

fn items() -> std::sync::MutexGuard<'static, Vec<Item>> {
    ITEMS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Removes the item when dropped.
#[must_use = "the item is removed as soon as the guard is dropped"]
pub struct Guard {
    id: u64,
}

impl Guard {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Changes the second line of the item (what it is doing now).
    pub fn set_detail(&self, detail: &str) {
        if let Some(i) = items().iter_mut().find(|i| i.id == self.id) {
            i.detail = detail.chars().take(200).collect();
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        items().retain(|i| i.id != self.id);
    }
}

fn cut(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>())
    }
}

/// Registers something that is running, for the tenant of the calling thread. A restricted tenant cannot register kinds other than
/// answers, memory jobs and clients (the item is then simply not shown; the guard still works).
pub fn start(kind: Kind, title: &str, detail: &str, source: &str, chat_id: Option<&str>, cancel: Option<Arc<AtomicBool>>) -> Guard {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let tenant = crate::tenant::current().map(|t| t.id);
    if crate::tenant::is_restricted() && !kind.for_restricted() {
        return Guard { id };
    }
    items().push(Item {
        id,
        kind,
        title: cut(title, 80),
        detail: cut(detail, 200),
        source: source.to_string(),
        chat_id: chat_id.filter(|c| !c.is_empty()).map(str::to_string),
        started_ms: now_ms(),
        cancel,
        tenant,
        term: None,
    });
    Guard { id }
}

/// May the calling thread see this item?
fn visible(i: &Item) -> bool {
    if crate::tenant::multi_tenant() {
        let Some(me) = crate::tenant::current() else { return false };
        if i.tenant.as_deref() != Some(me.id.as_str()) {
            return false;
        }
        if me.restricted && !i.kind.for_restricted() {
            return false;
        }
    }
    true
}

/// Things that are read from where they live: this computer's terminals and the sync.
fn live_items() -> Vec<Item> {
    let mut out = Vec::new();
    // a terminal and the sync belong to the person at the computer: never to a tenant of a server
    if crate::tenant::multi_tenant() || crate::tenant::is_restricted() {
        return out;
    }
    if crate::term::enabled() {
        for t in crate::term::list() {
            if !t.alive() {
                continue;
            }
            out.push(Item {
                id: TERM_BASE + t.id as u64,
                kind: Kind::Terminal,
                title: t.name(),
                detail: if t.by_ai { "Opened by the AI".into() } else { "Opened by you".into() },
                source: "desktop".into(),
                chat_id: None,
                started_ms: 0,
                cancel: None,
                tenant: None,
                term: Some(t.id),
            });
        }
    }
    let c = crate::sync::config();
    if c.enabled && !c.email.is_empty() && crate::sync::token().is_some() {
        let s = crate::sync::status();
        let detail = if s.running {
            format!("Syncing now ({})", c.email)
        } else if !s.error.is_empty() {
            format!("Problem: {}", cut(&s.error, 100))
        } else if s.last_ok > 0 {
            format!("{} · last sync {}", c.email, ago(now_ms().saturating_sub(s.last_ok)))
        } else {
            format!("{} · waiting for the first sync", c.email)
        };
        out.push(Item { id: SYNC_ID, kind: Kind::Sync, title: "Sync".into(), detail, source: "background".into(), chat_id: None, started_ms: 0, cancel: None, tenant: None, term: None });
    }
    out
}

/// "5 s ago", "2 min ago", "3 h ago".
pub fn ago(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s} s ago")
    } else if s < 3600 {
        format!("{} min ago", s / 60)
    } else {
        format!("{} h ago", s / 3600)
    }
}

/// Everything the calling thread may see, oldest first (always-on things last).
pub fn list() -> Vec<Item> {
    let mut v: Vec<Item> = items().iter().filter(|i| visible(i)).cloned().collect();
    v.extend(live_items());
    v
}

/// How many things are running, by kind name; the sync (always on) and the clients are counted too.
pub fn counts() -> Vec<(&'static str, usize)> {
    let all = list();
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    for i in &all {
        let k = i.kind.as_str();
        match out.iter_mut().find(|(n, _)| *n == k) {
            Some(e) => e.1 += 1,
            None => out.push((k, 1)),
        }
    }
    out
}

/// Things that are doing work for the person (not just present): what a "still running" warning is about.
pub fn busy_count() -> usize {
    list().iter().filter(|i| !matches!(i.kind, Kind::Sync | Kind::Client | Kind::Terminal)).count()
}

/// Stops one item: sets its cancel flag, or closes the terminal. False when there is no such item for this tenant, or it cannot be
/// stopped.
pub fn stop(id: u64) -> bool {
    if (TERM_BASE..TERM_BASE * 2).contains(&id) {
        if !live_items().iter().any(|i| i.id == id) {
            return false;
        }
        return match crate::term::get((id - TERM_BASE) as u32) {
            Some(t) => {
                t.close();
                true
            }
            None => false,
        };
    }
    let flag = items().iter().find(|i| i.id == id && visible(i)).and_then(|i| i.cancel.clone());
    match flag {
        Some(f) => {
            f.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

static SEEN: Mutex<Vec<(String, String, u64)>> = Mutex::new(Vec::new());

/// A client (phone, web, cli) just talked to the service: remembered so the app can say "phone link: seen 2 min ago".
pub fn seen(source: &str) {
    let tenant = crate::tenant::current().map(|t| t.id).unwrap_or_default();
    let mut s = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    match s.iter_mut().find(|(src, t, _)| src == source && *t == tenant) {
        Some(e) => e.2 = now_ms(),
        None => s.push((source.to_string(), tenant, now_ms())),
    }
}

/// When each kind of client last talked to the service, for the calling tenant: (source, ms since 1970).
pub fn clients_seen() -> Vec<(String, u64)> {
    let tenant = crate::tenant::current().map(|t| t.id).unwrap_or_default();
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(_, t, _)| *t == tenant).map(|(s, _, at)| (s.clone(), *at)).collect()
}

/// The `activity` service op.
pub fn to_json() -> Value {
    let seen: Vec<Value> = clients_seen().iter().map(|(s, at)| json!({"source": s, "last_ms": at})).collect();
    json!({"items": list().iter().map(Item::to_json).collect::<Vec<_>>(), "clients_seen": seen, "now_ms": now_ms()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenant::testkit::*;
    use crate::tenant::scope;

    fn mine(v: &[Item], title: &str) -> bool {
        v.iter().any(|i| i.title == title)
    }

    #[test]
    fn a_guard_removes_its_item_on_drop_early_return_and_panic() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        {
            let _a = start(Kind::Other, "guard-drop", "", "desktop", None, None);
            assert!(mine(&list(), "guard-drop"));
        }
        assert!(!mine(&list(), "guard-drop"));
        fn early(fail: bool) -> Result<(), String> {
            let _a = start(Kind::Other, "guard-early", "", "desktop", None, None);
            if fail {
                return Err("x".into());
            }
            Ok(())
        }
        assert!(early(true).is_err() && early(false).is_ok());
        assert!(!mine(&list(), "guard-early"));
        let r = std::panic::catch_unwind(|| {
            let _a = start(Kind::Other, "guard-panic", "", "desktop", None, None);
            panic!("boom");
        });
        assert!(r.is_err());
        assert!(!mine(&list(), "guard-panic"));
    }

    #[test]
    fn stop_sets_the_flag_only_for_items_that_have_one() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let flag = Arc::new(AtomicBool::new(false));
        let a = start(Kind::Answer, "stoppable", "", "phone", Some("c1"), Some(flag.clone()));
        let b = start(Kind::Command, "watch only", "", "desktop", None, None);
        assert!(!stop(b.id()));
        assert!(stop(a.id()));
        assert!(flag.load(Ordering::SeqCst));
        assert!(!stop(u64::MAX));
        let l = list();
        assert!(l.iter().find(|i| i.id == a.id()).unwrap().can_stop());
        assert!(!l.iter().find(|i| i.id == b.id()).unwrap().can_stop());
        assert!(l.iter().find(|i| i.id == a.id()).unwrap().to_json()["chat_id"] == "c1");
    }

    #[test]
    fn long_titles_are_cut_and_made_one_line() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let g = start(Kind::Command, &format!("echo a\n  b {}", "x".repeat(200)), "", "desktop", None, None);
        let t = list().into_iter().find(|i| i.id == g.id()).unwrap().title;
        assert!(t.starts_with("echo a b x") && t.chars().count() <= 81 && !t.contains('\n'), "{t}");
    }

    #[test]
    fn tenants_cannot_see_or_stop_each_others_items() {
        let _g = crate::store::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        env_once();
        crate::tenant::test_multi(true);
        let (a, b, r) = (fresh(false), fresh(false), fresh(true));
        let (fa, fb, fr) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let ga = scope(&a, || start(Kind::Answer, "a-answer", "", "web", None, Some(fa.clone())));
        let gb = scope(&b, || start(Kind::Command, "b-command", "", "desktop", None, Some(fb.clone())));
        let gr = scope(&r, || start(Kind::Answer, "r-answer", "", "web", None, Some(fr.clone())));
        // a restricted tenant cannot have a command or a terminal in the list, whatever the code asks for
        let gr2 = scope(&r, || start(Kind::Command, "r-command", "", "web", None, None));
        assert!(scope(&a, list).iter().all(|i| i.title == "a-answer"));
        assert!(scope(&b, list).iter().all(|i| i.title == "b-command"));
        assert_eq!(scope(&r, list).iter().map(|i| i.title.clone()).collect::<Vec<_>>(), ["r-answer"]);
        assert!(scope(&r, || !stop(gb.id())) && !fb.load(Ordering::SeqCst));
        assert!(scope(&b, || !stop(ga.id())) && !fa.load(Ordering::SeqCst));
        assert!(scope(&a, || !stop(gr.id())) && !fr.load(Ordering::SeqCst));
        // without a tenant a multi-tenant server shows nothing
        assert!(list().is_empty() && !stop(ga.id()));
        assert!(scope(&a, || stop(ga.id())) && fa.load(Ordering::SeqCst));
        assert!(scope(&r, || stop(gr.id())) && fr.load(Ordering::SeqCst));
        // the sources seen are kept per tenant too
        scope(&a, || seen("phone"));
        assert_eq!(scope(&a, clients_seen).len(), 1);
        assert!(scope(&b, clients_seen).is_empty());
        drop((ga, gb, gr, gr2));
        crate::tenant::test_multi(false);
        assert!(scope(&a, list).is_empty());
    }
}
