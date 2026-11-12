//! The server's memory of the people who are using it: one `Entry` per person (their folders, their rate bucket, how many answers
//! they have running, their Firestore sync), the registry that caps how many people there are, and the rules for fetching a person
//! lazily, sending their changes, taking idle people off the disk and deleting an account completely.
//!
//! Lock order (never the other way round): `entries` -> `deleted`; `entries` -> an entry's `state`. An entry's `sync` lock is taken
//! without holding `entries` or `state` (the Firestore calls inside it are slow), and `state` is never held while taking `sync`.

use crate::db::Db;
use crate::sync::Sync;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zo_core::tenant::Tenant;

pub const RATE_PER_MIN: u32 = 60;
pub const SENDS_PER_TENANT: usize = 2;
/// Requests of one person being served at the same time (a request that is slow to send its body counts too).
pub const ACTIVE_PER_TENANT: usize = 8;
/// Desktop-sync requests (bodies up to 20 MB, parsed in memory) at the same time: per person and on the whole server.
pub const SYNCS_PER_TENANT: usize = 2;
pub const SYNCS_GLOBAL: usize = 6;
/// A person who is not registered may knock this often a minute (each knock costs a Firestore read).
pub const UNREGISTERED_PER_MIN: u32 = 10;
/// When the registry said "full", people who are not registered are refused for this long without asking Firestore again.
pub const FULL_MEMORY: Duration = Duration::from_secs(30);
/// A person nobody has asked for in this long, with everything already in Firestore, is taken off the disk.
pub const IDLE_EVICT: Duration = Duration::from_secs(30 * 60);
/// After an account is deleted, requests for it are refused for this long (so nothing running can bring it back).
pub const DELETE_COOLDOWN: Duration = Duration::from_secs(2 * 3600);
/// The record of a deleted account (collection `deleted`): kept in Firestore, so it survives a restart. A sign-in token lives about an
/// hour, so a deleted person's old tabs are refused for `DELETE_COOLDOWN`; if their login could not be removed, for good.
pub const TOMBSTONES: &str = "deleted";
pub const REGISTRY: &str = "registry";

/// Why a person is not served right now.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Too many requests: try again in this many seconds.
    Rate(u64),
    /// Too many answers running (theirs, or the server's): try again in this many seconds.
    Busy(&'static str, u64),
    /// The server has as many people as it takes.
    Full,
    /// The account was deleted a moment ago.
    Gone,
    /// Firestore (or the disk) failed.
    Storage(String),
}

// ---------------------------------------------------------------------------------------------------------------------------
// the rate bucket

/// A token bucket: `per_min` requests a minute, a burst of as many.
pub struct Bucket {
    tokens: f64,
    at: Instant,
    cap: f64,
    per_sec: f64,
}

impl Bucket {
    pub fn new(per_min: u32, now: Instant) -> Bucket {
        Bucket { tokens: per_min as f64, at: now, cap: per_min as f64, per_sec: per_min as f64 / 60.0 }
    }

    /// Takes one request, or says in how many seconds there is room again.
    pub fn take(&mut self, now: Instant) -> Result<(), u64> {
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + dt * self.per_sec).min(self.cap);
        self.at = self.at.max(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err((((1.0 - self.tokens) / self.per_sec).ceil() as u64).max(1))
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// the registry: who has an account, and the cap

/// May one more person register, when `count` have?
pub fn allow_new(count: usize, max: usize) -> bool {
    count < max
}

pub struct Registry {
    db: Arc<dyn Db>,
    max: usize,
    known: Mutex<HashSet<String>>,
    /// Registrations go one at a time inside this process, so the count cannot be raced past the cap here (another server process
    /// could add a few at the same moment).
    gate: Mutex<()>,
    /// When the registry was last found full.
    full_at: Mutex<Option<Instant>>,
}

impl Registry {
    pub fn new(db: Arc<dyn Db>, max: usize) -> Registry {
        Registry { db, max, known: Mutex::new(HashSet::new()), gate: Mutex::new(()), full_at: Mutex::new(None) }
    }

    pub fn is_known(&self, tenant: &str) -> bool {
        self.known.lock().unwrap().contains(tenant)
    }

    /// Makes sure the person has a place, or refuses with `Full`. Someone who is registered stays served even when the server is full.
    pub fn ensure(&self, tenant: &str) -> Result<(), Refusal> {
        if self.known.lock().unwrap().contains(tenant) {
            return Ok(());
        }
        let st = |e: String| Refusal::Storage(e);
        // one read, outside the gate: somebody who has a place is the common case, and must not wait behind the others
        if self.db.get_doc(REGISTRY, tenant).map_err(st)?.is_some() {
            self.known.lock().unwrap().insert(tenant.to_string());
            return Ok(());
        }
        // a deleted account is not brought back by a token that was still valid (written by `delete_account`, see account.rs)
        if let Some(t) = self.db.get_doc(TOMBSTONES, tenant).map_err(Refusal::Storage)? {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
            let at = v.get("at").and_then(|a| a.as_u64()).unwrap_or(0);
            let login_removed = v.get("login_removed").and_then(|a| a.as_bool()).unwrap_or(false);
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            if !login_removed || now.saturating_sub(at) < DELETE_COOLDOWN.as_secs() {
                return Err(Refusal::Gone);
            }
            let _ = self.db.delete_doc(TOMBSTONES, tenant); // old enough: the marker goes too
        }
        if self.full_at.lock().unwrap().is_some_and(|t| t.elapsed() < FULL_MEMORY) {
            return Err(Refusal::Full); // found full a moment ago: no second look at the whole registry
        }
        let _g = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let count = self.db.list_docs(REGISTRY).map_err(Refusal::Storage)?.len();
        if !allow_new(count, self.max) {
            *self.full_at.lock().unwrap() = Some(Instant::now());
            return Err(Refusal::Full);
        }
        let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.db.put_doc(REGISTRY, tenant, &json!({"created": created}).to_string()).map_err(Refusal::Storage)?;
        self.known.lock().unwrap().insert(tenant.to_string());
        Ok(())
    }

    /// Gives the place back (an account was deleted).
    pub fn remove(&self, tenant: &str) -> Result<(), String> {
        let _g = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        self.db.delete_doc(REGISTRY, tenant)?;
        self.known.lock().unwrap().remove(tenant);
        *self.full_at.lock().unwrap() = None; // there is a place again
        Ok(())
    }

    pub fn count(&self) -> Result<usize, String> {
        Ok(self.db.list_docs(REGISTRY)?.len())
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// one person

struct State {
    bucket: Bucket,
    last_used: Instant,
    /// Requests being served now (a running answer counts too).
    active: usize,
    /// Answers running now.
    sends: usize,
    /// Sync requests being handled now.
    syncs: usize,
    /// Taken off the map (evicted or deleted): whoever holds it must ask the map again.
    gone: bool,
}

pub struct Entry {
    pub id: String,
    base: PathBuf,
    state: Mutex<State>,
    /// None when there is no Firestore (the local test mode): the disk is then the only store.
    sync: Mutex<Option<Sync>>,
}

/// Evict only a person nobody is using, who has been left alone long enough.
pub fn can_evict(idle: Duration, active: usize, running_memory_job: bool) -> bool {
    idle >= IDLE_EVICT && active == 0 && !running_memory_job
}

pub struct TenantMap {
    root: PathBuf,
    db: Option<Arc<dyn Db>>,
    secret: String,
    reg: Option<Registry>,
    entries: Mutex<HashMap<String, Arc<Entry>>>,
    deleted: Mutex<HashMap<String, Instant>>,
    runs: AtomicUsize,
    syncs: AtomicUsize,
    max_runs: usize,
    per_min: u32,
    /// People who are not registered yet: their own small buckets, checked before anything costs a Firestore read.
    knocks: Mutex<HashMap<String, Bucket>>,
}

/// One request being served for one person. Dropping it frees their place.
pub struct Lease {
    entry: Arc<Entry>,
    map: Arc<TenantMap>,
}

/// One answer running. Dropping it (the answer ended, or the person went away) frees the place.
pub struct SendPermit {
    entry: Arc<Entry>,
    map: Arc<TenantMap>,
}

impl TenantMap {
    pub fn new(root: PathBuf, db: Option<Arc<dyn Db>>, secret: String, max_users: usize, max_runs: usize) -> TenantMap {
        let reg = db.clone().map(|d| Registry::new(d, max_users));
        TenantMap { root, db, secret, reg, entries: Mutex::new(HashMap::new()), deleted: Mutex::new(HashMap::new()), runs: AtomicUsize::new(0), syncs: AtomicUsize::new(0), max_runs, per_min: RATE_PER_MIN, knocks: Mutex::new(HashMap::new()) }
    }

    #[cfg(test)]
    pub fn with_rate(mut self, per_min: u32) -> TenantMap {
        self.per_min = per_min;
        self
    }

    pub fn per_min(&self) -> u32 {
        self.per_min
    }

    pub fn registry(&self) -> Option<&Registry> {
        self.reg.as_ref()
    }

    pub fn db(&self) -> Option<&Arc<dyn Db>> {
        self.db.as_ref()
    }

    pub fn running_answers(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }

    pub fn loaded_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    fn is_deleted_now(&self, id: &str) -> bool {
        self.deleted.lock().unwrap().get(id).is_some_and(|t| t.elapsed() < DELETE_COOLDOWN)
    }

    fn entry(&self, id: &str) -> Result<Arc<Entry>, Refusal> {
        let mut map = self.entries.lock().unwrap();
        {
            let mut d = self.deleted.lock().unwrap();
            match d.get(id) {
                Some(t) if t.elapsed() < DELETE_COOLDOWN => return Err(Refusal::Gone),
                Some(_) => {
                    // the cooldown is over: whatever a straggler wrote to the disk meanwhile goes too
                    d.remove(id);
                    let _ = std::fs::remove_dir_all(self.root.join(id));
                }
                None => {}
            }
        }
        if let Some(e) = map.get(id) {
            return Ok(e.clone());
        }
        let t = Tenant::open_in(&self.root, id, true).map_err(Refusal::Storage)?;
        let sync = self.db.as_ref().map(|db| Sync::new(db.clone(), self.secret.clone(), id, t.config.clone(), t.data.clone()));
        let now = Instant::now();
        let e = Arc::new(Entry {
            id: id.to_string(),
            base: self.root.join(id),
            state: Mutex::new(State { bucket: Bucket::new(self.per_min, now), last_used: now, active: 0, sends: 0, syncs: 0, gone: false }),
            sync: Mutex::new(sync),
        });
        map.insert(id.to_string(), e.clone());
        Ok(e)
    }

    /// Serves a request for a person: refuses when they are over their rate, makes sure they have a place (unless admin), and
    /// fetches their data from Firestore the first time. Nothing of theirs is served before that has worked.
    pub fn acquire(self: &Arc<Self>, id: &str, admin: bool, now: Instant) -> Result<Lease, Refusal> {
        if self.is_deleted_now(id) {
            return Err(Refusal::Gone);
        }
        let registered = match (&self.reg, admin) {
            (Some(r), false) => {
                if !r.is_known(id) {
                    self.knock(id, now)?;
                }
                r.ensure(id)?;
                true
            }
            _ => false,
        };
        for _ in 0..3 {
            let e = match self.entry(id) {
                Ok(e) => e,
                Err(Refusal::Gone) => {
                    if registered {
                        // deleted while we were registering: do not leave a place taken by nobody
                        if let Some(r) = &self.reg {
                            let _ = r.remove(id);
                        }
                    }
                    return Err(Refusal::Gone);
                }
                Err(e) => return Err(e),
            };
            {
                let mut st = e.state.lock().unwrap();
                if st.gone {
                    continue;
                }
                if st.active >= ACTIVE_PER_TENANT {
                    return Err(Refusal::Busy("You have too many requests running at once. Wait for some to finish.", 3));
                }
                st.bucket.take(now).map_err(Refusal::Rate)?;
                st.active += 1;
                st.last_used = now;
            }
            let lease = Lease { entry: e, map: self.clone() };
            lease.entry.load()?;
            return Ok(lease);
        }
        Err(Refusal::Gone)
    }

    /// A person who is not known to this process yet asks to be let in (a first request, or one after a restart, or someone who is
    /// refused): limited per person, so that none can make the server read Firestore without end.
    fn knock(&self, id: &str, now: Instant) -> Result<(), Refusal> {
        let mut k = self.knocks.lock().unwrap();
        if k.len() > 20_000 {
            k.clear(); // bounded memory; a full reset only gives everybody a fresh small allowance
        }
        k.entry(id.to_string()).or_insert_with(|| Bucket::new(UNREGISTERED_PER_MIN, now)).take(now).map_err(Refusal::Rate)
    }

    /// One pass of the push loop: every fetched person's changes go to Firestore. The number of people that failed.
    pub fn push_all(&self) -> usize {
        let all: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
        let mut failed = 0;
        for e in all {
            if e.state.lock().unwrap().gone {
                continue;
            }
            // somebody else is working with their sync (a first fetch, or a delete): next round
            let Ok(mut g) = e.sync.try_lock() else { continue };
            if let Some(s) = g.as_mut() {
                if let Err(err) = s.push() {
                    eprintln!("zo-server: {err}");
                    failed += 1;
                }
            }
        }
        failed
    }

    /// Takes people off the disk who have been idle (see `can_evict`) and have nothing unsent. Returns how many.
    pub fn evict_idle(&self, now: Instant) -> usize {
        let all: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
        let mut n = 0;
        for e in all {
            if e.sync.lock().map(|g| g.is_none()).unwrap_or(true) {
                continue; // no Firestore: the disk is all there is
            }
            let probe = |st: &State| can_evict(now.saturating_duration_since(st.last_used), st.active, false);
            if !probe(&e.state.lock().unwrap()) {
                continue;
            }
            let memory_job = {
                let t = Tenant { id: e.id.clone(), config: e.base.join("config"), data: e.base.join("data"), restricted: true };
                zo_core::tenant::scope(&t, zo_core::ai::memory_agent::is_running)
            };
            if memory_job {
                continue;
            }
            // everything must be in Firestore first
            {
                let Ok(mut g) = e.sync.try_lock() else { continue };
                let Some(s) = g.as_mut() else { continue };
                if !s.loaded || s.push().is_err() || !s.is_clean() {
                    continue;
                }
            }
            // the folder goes while the map is held, so nobody can fetch the same person again before it is gone
            let mut map = self.entries.lock().unwrap();
            let mut st = e.state.lock().unwrap();
            if !probe(&st) || !map.get(&e.id).is_some_and(|m| Arc::ptr_eq(m, &e)) {
                continue;
            }
            st.gone = true;
            let _ = std::fs::remove_dir_all(&e.base);
            map.remove(&e.id);
            n += 1;
        }
        n
    }

    /// Deletes everything of one person. `close_streams` stops their running answers, `remote` removes what is outside this
    /// process (Firestore: documents, device index, registry slot). The person is refused from the first step on and stays refused
    /// for `DELETE_COOLDOWN`, nothing of theirs is written to Firestore again, and the local folder and memory are removed even if
    /// `remote` fails (the person is then told to try again, which finds what is left). Running it again is harmless.
    pub fn delete_account(&self, lease: &Lease, close_streams: impl FnOnce(), remote: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let id = lease.entry.id.clone();
        {
            let _map = self.entries.lock().unwrap();
            self.deleted.lock().unwrap().insert(id.clone(), Instant::now());
        }
        lease.entry.state.lock().unwrap().gone = true;
        close_streams();
        // the running requests end (this one counts as one)
        let t0 = Instant::now();
        while lease.entry.state.lock().unwrap().active > 1 && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        // after this, a push that was under way is over and no later one can start
        if let Some(s) = lease.entry.sync.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            s.dead = true;
        }
        let r = remote();
        let mut map = self.entries.lock().unwrap();
        map.remove(&id);
        let _ = std::fs::remove_dir_all(&lease.entry.base);
        if r.is_err() {
            self.deleted.lock().unwrap().remove(&id); // so that the person can try again
        }
        r
    }
}

impl Entry {
    fn load(&self) -> Result<(), Refusal> {
        let mut g = self.sync.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_mut() {
            None => Ok(()),
            Some(s) if s.dead => Err(Refusal::Gone),
            Some(s) if s.loaded => Ok(()),
            Some(s) => s.pull().map(|_| ()).map_err(|e| {
                eprintln!("zo-server: cannot read one account's data from Firestore, so it is not served: {e}");
                Refusal::Storage("The storage is not reachable right now. Try again in a moment.".into())
            }),
        }
    }
}

impl Lease {
    pub fn id(&self) -> &str {
        &self.entry.id
    }

    /// The person as zo-core knows them.
    pub fn tenant(&self, restricted: bool) -> Tenant {
        Tenant { id: self.entry.id.clone(), config: self.entry.base.join("config"), data: self.entry.base.join("data"), restricted }
    }

    /// Reserves one running answer: at most 2 per person and `ZO_MAX_RUNS` on the whole server.
    pub fn permit_send(&self) -> Result<SendPermit, Refusal> {
        {
            let mut st = self.entry.state.lock().unwrap();
            if st.sends >= SENDS_PER_TENANT {
                return Err(Refusal::Busy("You already have two answers being written. Wait for one to finish.", 5));
            }
            st.sends += 1;
            st.active += 1;
        }
        let m = &self.map;
        #[allow(deprecated)]
        let reserved = m.runs.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < m.max_runs).then_some(n + 1));
        if reserved.is_err() {
            let mut st = self.entry.state.lock().unwrap();
            st.sends -= 1;
            st.active -= 1;
            return Err(Refusal::Busy("The server is answering as many people as it can right now. Try again in a moment.", 10));
        }
        Ok(SendPermit { entry: self.entry.clone(), map: self.map.clone() })
    }
}

/// One sync request being handled.
pub struct SyncPermit {
    entry: Arc<Entry>,
    map: Arc<TenantMap>,
}

impl Lease {
    /// Reserves one sync request: at most `SYNCS_PER_TENANT` per person and `SYNCS_GLOBAL` on the whole server (their bodies are big).
    pub fn permit_sync(&self) -> Result<SyncPermit, Refusal> {
        let m = &self.map;
        #[allow(deprecated)]
        let reserved = m.syncs.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < SYNCS_GLOBAL).then_some(n + 1));
        if reserved.is_err() {
            return Err(Refusal::Busy("The server is syncing for many people right now. Try again in a moment.", 5));
        }
        let mut st = self.entry.state.lock().unwrap();
        if st.syncs >= SYNCS_PER_TENANT {
            drop(st);
            m.syncs.fetch_sub(1, Ordering::SeqCst);
            return Err(Refusal::Busy("Two syncs are already running. Wait for one to finish.", 3));
        }
        st.syncs += 1;
        Ok(SyncPermit { entry: self.entry.clone(), map: self.map.clone() })
    }
}

impl Drop for SyncPermit {
    fn drop(&mut self) {
        self.map.syncs.fetch_sub(1, Ordering::SeqCst);
        let mut st = self.entry.state.lock().unwrap_or_else(|e| e.into_inner());
        st.syncs = st.syncs.saturating_sub(1);
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut st = self.entry.state.lock().unwrap_or_else(|e| e.into_inner());
        st.active = st.active.saturating_sub(1);
        st.last_used = Instant::now();
    }
}

impl Drop for SendPermit {
    fn drop(&mut self) {
        self.map.runs.fetch_sub(1, Ordering::SeqCst);
        let mut st = self.entry.state.lock().unwrap_or_else(|e| e.into_inner());
        st.sends = st.sends.saturating_sub(1);
        st.active = st.active.saturating_sub(1);
        st.last_used = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemDb;

    fn root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zo-tm-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn id(n: usize) -> String {
        format!("{n:032x}")
    }

    fn map(tag: &str, db: Arc<MemDb>, max_users: usize, max_runs: usize) -> Arc<TenantMap> {
        Arc::new(TenantMap::new(root(tag), Some(db), "sec".into(), max_users, max_runs))
    }

    #[test]
    fn the_bucket_allows_a_burst_then_the_rate_and_says_when_to_come_back() {
        let t0 = Instant::now();
        let mut b = Bucket::new(60, t0);
        for _ in 0..60 {
            assert!(b.take(t0).is_ok());
        }
        assert_eq!(b.take(t0), Err(1));
        assert!(b.take(t0 + Duration::from_millis(500)).is_err());
        assert!(b.take(t0 + Duration::from_secs(1)).is_ok(), "one request a second comes back");
        assert!(b.take(t0 + Duration::from_secs(1)).is_err());
        // after a long pause the burst is back, but never more than 60
        let later = t0 + Duration::from_secs(3600);
        for _ in 0..60 {
            assert!(b.take(later).is_ok());
        }
        assert!(b.take(later).is_err());
        let mut slow = Bucket::new(6, t0);
        for _ in 0..6 {
            slow.take(t0).unwrap();
        }
        assert_eq!(slow.take(t0), Err(10));
    }

    #[test]
    fn the_cap_logic() {
        assert!(allow_new(0, 200) && allow_new(199, 200) && !allow_new(200, 200) && !allow_new(500, 200));
    }

    #[test]
    fn the_201st_person_is_refused_registered_people_stay_and_a_deleted_place_is_free_again() {
        let db = Arc::new(MemDb::default());
        let reg = Registry::new(db.clone(), 200);
        for n in 0..200 {
            reg.ensure(&id(n)).unwrap();
        }
        assert_eq!(reg.count().unwrap(), 200);
        assert_eq!(reg.ensure(&id(200)), Err(Refusal::Full));
        assert_eq!(reg.count().unwrap(), 200, "a refused person takes no place");
        reg.ensure(&id(5)).unwrap();
        // a fresh process (empty memory) still knows who is registered, from Firestore
        let again = Registry::new(db.clone(), 200);
        again.ensure(&id(7)).unwrap();
        assert_eq!(again.ensure(&id(201)), Err(Refusal::Full));
        reg.remove(&id(3)).unwrap();
        reg.ensure(&id(200)).unwrap();
        assert_eq!(reg.count().unwrap(), 200);
    }

    #[test]
    fn a_race_of_registrations_does_not_pass_the_cap() {
        let db = Arc::new(MemDb::default());
        let reg = Arc::new(Registry::new(db.clone(), 10));
        let hs: Vec<_> = (0..40).map(|n| { let r = reg.clone(); std::thread::spawn(move || r.ensure(&id(n)).is_ok()) }).collect();
        let ok = hs.into_iter().filter_map(|h| h.join().ok()).filter(|b| *b).count();
        assert_eq!(ok, 10);
        assert_eq!(reg.count().unwrap(), 10);
    }

    #[test]
    fn admins_do_not_take_a_place_and_a_full_server_makes_no_folder_for_a_refused_person() {
        let db = Arc::new(MemDb::default());
        let m = map("cap", db.clone(), 1, 8);
        let now = Instant::now();
        drop(m.acquire(&id(1), false, now).unwrap());
        assert_eq!(m.acquire(&id(2), false, now).err(), Some(Refusal::Full));
        assert!(!m.root.join(id(2)).exists());
        drop(m.acquire(&id(3), true, now).unwrap());
        assert_eq!(m.registry().unwrap().count().unwrap(), 1);
    }

    #[test]
    fn each_person_has_their_own_bucket() {
        let db = Arc::new(MemDb::default());
        let m = Arc::new(TenantMap::new(root("rate"), Some(db), "s".into(), 200, 8).with_rate(3));
        let now = Instant::now();
        for _ in 0..3 {
            drop(m.acquire(&id(1), false, now).unwrap());
        }
        assert!(matches!(m.acquire(&id(1), false, now), Err(Refusal::Rate(s)) if s >= 1));
        drop(m.acquire(&id(2), false, now).unwrap());
    }

    #[test]
    fn two_answers_per_person_and_the_global_limit_and_permits_come_back() {
        let db = Arc::new(MemDb::default());
        let m = map("runs", db, 200, 3);
        let now = Instant::now();
        let a = m.acquire(&id(1), false, now).unwrap();
        let p1 = a.permit_send().unwrap();
        let p2 = a.permit_send().unwrap();
        assert!(matches!(a.permit_send(), Err(Refusal::Busy(..))), "the third answer of one person");
        let b = m.acquire(&id(2), false, now).unwrap();
        let _q1 = b.permit_send().unwrap();
        assert!(matches!(b.permit_send(), Err(Refusal::Busy(..))), "the server's own limit of 3 is reached");
        assert_eq!(m.running_answers(), 3);
        drop(p1);
        assert_eq!(m.running_answers(), 2);
        let _q2 = b.permit_send().unwrap();
        drop(p2);
        assert!(a.permit_send().is_ok());
    }

    #[test]
    fn a_person_is_fetched_once_before_anything_and_a_failed_fetch_serves_nothing() {
        let db = Arc::new(MemDb::default());
        let t = id(9);
        db.put_doc(&format!("users/{t}/files"), "memory_state", r#"{"enabled":true,"every":4}"#).unwrap();
        let m = map("fetch", db.clone(), 200, 8);
        db.fail.store(true, Ordering::SeqCst);
        assert!(matches!(m.acquire(&t, true, Instant::now()), Err(Refusal::Storage(_))));
        db.fail.store(false, Ordering::SeqCst);
        let l = m.acquire(&t, true, Instant::now()).unwrap();
        assert!(l.tenant(true).data.join("memory.json").exists(), "the saved memory state came from Firestore");
    }

    #[test]
    fn eviction_rules() {
        let m = IDLE_EVICT;
        assert!(can_evict(m, 0, false));
        assert!(!can_evict(m - Duration::from_secs(1), 0, false), "not idle long enough");
        assert!(!can_evict(m * 2, 1, false), "a request is being served");
        assert!(!can_evict(m * 2, 0, true), "a memory job is running");
    }

    #[test]
    fn an_idle_person_with_nothing_unsent_is_taken_off_the_disk_and_comes_back_from_firestore() {
        let db = Arc::new(MemDb::default());
        let m = map("evict", db.clone(), 200, 8);
        let t = id(4);
        let t0 = Instant::now();
        let l = m.acquire(&t, false, t0).unwrap();
        let data = l.tenant(true).data;
        std::fs::write(data.join("chats.json"), r#"[{"id":"c1","title":"hello"}]"#).unwrap();
        drop(l);
        let late = Instant::now() + IDLE_EVICT + Duration::from_secs(5);
        assert_eq!(m.evict_idle(Instant::now()), 0, "not idle yet");
        // the change was never sent, but eviction sends it first
        assert_eq!(m.evict_idle(late), 1);
        assert_eq!(m.loaded_count(), 0);
        assert!(!m.root.join(&t).exists(), "the folder is gone");
        assert!(db.get_doc(&format!("users/{t}/chats"), "c1").unwrap().is_some(), "and what was on it is in Firestore");
        let l = m.acquire(&t, false, late).unwrap();
        assert!(std::fs::read_to_string(l.tenant(true).data.join("chats.json")).unwrap().contains("hello"));
    }

    #[test]
    fn a_person_whose_changes_cannot_be_sent_is_not_evicted() {
        let db = Arc::new(MemDb::default());
        let m = map("evict-fail", db.clone(), 200, 8);
        let t = id(5);
        let l = m.acquire(&t, false, Instant::now()).unwrap();
        std::fs::write(l.tenant(true).data.join("chats.json"), r#"[{"id":"c1","title":"unsent"}]"#).unwrap();
        drop(l);
        db.fail.store(true, Ordering::SeqCst);
        assert_eq!(m.evict_idle(Instant::now() + IDLE_EVICT * 2), 0);
        assert!(m.root.join(&t).exists());
        db.fail.store(false, Ordering::SeqCst);
        assert_eq!(m.evict_idle(Instant::now() + IDLE_EVICT * 2), 1);
    }

    #[test]
    fn someone_busy_is_not_evicted() {
        let db = Arc::new(MemDb::default());
        let m = map("evict-busy", db, 200, 8);
        let l = m.acquire(&id(6), false, Instant::now()).unwrap();
        assert_eq!(m.evict_idle(Instant::now() + IDLE_EVICT * 2), 0);
        drop(l);
    }

    #[test]
    fn delete_removes_everything_of_one_person_refuses_them_afterwards_and_stops_the_push() {
        let db = Arc::new(MemDb::default());
        let m = map("del", db.clone(), 200, 8);
        let (a, b) = (id(10), id(11));
        for t in [&a, &b] {
            let l = m.acquire(t, false, Instant::now()).unwrap();
            std::fs::write(l.tenant(true).data.join("chats.json"), format!(r#"[{{"id":"c-{t}","title":"x"}}]"#)).unwrap();
        }
        m.push_all();
        assert!(db.count_under(&format!("users/{a}")) > 0 && db.count_under(&format!("users/{b}")) > 0);
        let la = m.acquire(&a, false, Instant::now()).unwrap();
        let dbc = db.clone();
        let (ac, reg) = (a.clone(), m.registry().unwrap() as *const Registry as usize);
        let r = m.delete_account(&la, || {}, move || {
            // SAFETY: the registry lives as long as the map, which outlives this call
            let reg = unsafe { &*(reg as *const Registry) };
            crate::db::wipe_tree(&*dbc, &format!("users/{ac}"), &["chats", "memory", "files", "meta"])?;
            reg.remove(&ac)
        });
        assert!(r.is_ok());
        assert_eq!(db.count_under(&format!("users/{a}")), 0);
        assert!(db.count_under(&format!("users/{b}")) > 0, "the other person is untouched");
        assert!(db.get_doc(REGISTRY, &a).unwrap().is_none() && db.get_doc(REGISTRY, &b).unwrap().is_some());
        assert!(!m.root.join(&a).exists() && m.root.join(&b).exists());
        // the push loop cannot bring anything back, even for the lease that is still held
        std::fs::create_dir_all(la.tenant(true).data).unwrap();
        std::fs::write(la.tenant(true).data.join("chats.json"), r#"[{"id":"zombie","title":"x"}]"#).unwrap();
        m.push_all();
        assert_eq!(db.count_under(&format!("users/{a}")), 0);
        // and the person is refused, without making a place or a folder
        assert_eq!(m.acquire(&a, false, Instant::now()).err(), Some(Refusal::Gone));
        assert!(db.get_doc(REGISTRY, &a).unwrap().is_none());
        // the cooldown ends: they may begin again, from nothing
        m.deleted.lock().unwrap().insert(a.clone(), Instant::now() - DELETE_COOLDOWN - Duration::from_secs(1));
        drop(la);
        let fresh = m.acquire(&a, false, Instant::now()).unwrap();
        assert!(!fresh.tenant(true).data.join("chats.json").exists(), "the straggler's file was removed too");
    }

    #[test]
    fn a_failed_remote_delete_can_be_run_again_and_finishes() {
        let db = Arc::new(MemDb::default());
        let m = map("del-retry", db.clone(), 200, 8);
        let a = id(12);
        let l = m.acquire(&a, true, Instant::now()).unwrap();
        db.put_doc(&format!("users/{a}/chats"), "c1", "{}").unwrap();
        let r = m.delete_account(&l, || {}, || Err("Firestore is down".into()));
        assert!(r.is_err());
        drop(l);
        let l = m.acquire(&a, true, Instant::now()).expect("not stuck in the cooldown after a failure");
        let dbc = db.clone();
        let ac = a.clone();
        assert!(m.delete_account(&l, || {}, move || crate::db::wipe_tree(&*dbc, &format!("users/{ac}"), &["chats"]).map(|_| ())).is_ok());
        assert_eq!(db.count_under(&format!("users/{a}")), 0);
    }

    #[test]
    fn a_person_who_is_refused_cannot_make_the_server_read_the_registry_over_and_over() {
        let db = Arc::new(MemDb::default());
        let m = map("knock", db.clone(), 2, 8);
        let now = Instant::now();
        for n in 0..2 {
            m.acquire(&id(n), false, now).unwrap();
        }
        // the registry is full: the third person is refused, and after a few knocks is told to wait (no database read at all)
        let mut rate = false;
        for _ in 0..30 {
            match m.acquire(&id(9), false, now) {
                Err(Refusal::Rate(_)) => rate = true,
                Err(Refusal::Full) => {}
                other => panic!("{:?}", other.err()),
            }
        }
        assert!(rate, "refused knocks are limited per person");
        // found full a moment ago: another stranger is refused without a second look at the whole registry
        let reads = db.reads.load(Ordering::SeqCst);
        assert_eq!(m.acquire(&id(10), false, now).err(), Some(Refusal::Full));
        assert_eq!(db.reads.load(Ordering::SeqCst), reads, "no listing of the whole registry");
    }

    #[test]
    fn one_person_cannot_hold_more_than_a_few_requests_or_syncs_at_once() {
        let db = Arc::new(MemDb::default());
        let m = map("active", db, 200, 8);
        let now = Instant::now();
        let held: Vec<Lease> = (0..ACTIVE_PER_TENANT).map(|_| m.acquire(&id(1), false, now).unwrap()).collect();
        assert!(matches!(m.acquire(&id(1), false, now), Err(Refusal::Busy(..))), "stuck requests cannot pile up without end");
        assert!(m.acquire(&id(2), false, now).is_ok(), "another person is not affected");
        let (a, b) = (held[0].permit_sync().unwrap(), held[1].permit_sync().unwrap());
        assert!(held[2].permit_sync().is_err(), "two syncs per person");
        drop(a);
        assert!(held[2].permit_sync().is_ok());
        drop(b);
    }
}
