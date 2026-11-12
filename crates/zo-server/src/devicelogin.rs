//! The browser device login: how the desktop app and the `zo` client get signed in without anybody copying a secret.
//!
//! 1. The device calls `device_start` (no login) and gets a long secret `device_code` (kept on the device, never shown) and a short
//!    `user_code` ("ABCD-EFGH") to show the person.
//! 2. The person opens `<public base>/device` in a browser, signs in (Google or e-mail), TYPES the user code, sees what is being
//!    approved (name, kind, requesting address) and approves or denies. The link never carries the code.
//! 3. The device polls `device_poll` with its device_code and gets its credential exactly once.
//!
//! This file is the store of waiting requests. It is in memory (a request lives ten minutes; a restart only makes people start
//! again), with hard caps: requests per source address per minute, waiting requests per address, waiting requests on the whole
//! server. The device_code is never stored: only its sha256 is the key (so a lookup does not compare secrets byte by byte, and a memory
//! dump does not hand out working codes). Wrong user codes are limited per person (guessing a code must not be possible), a code is
//! single use, and a credential that was approved but never collected is revoked when its request runs out.
//!
//! Every method takes `now` so the tests can move the clock (`DeviceLogin::now`).

use crate::devices::{hash_token, random_bytes, Kind};
use crate::tenants::Bucket;
use base64::Engine;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const EXPIRES_SECS: u64 = 600;
pub const INTERVAL_SECS: u64 = 3;
/// A device may ask a little earlier than the interval (its timer and the network are not exact); faster is "slow down".
const MIN_POLL_GAP: Duration = Duration::from_millis(2500);
/// Letters and digits that cannot be mistaken for each other (no 0/O, 1/I/L).
const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// Wrong codes one person may type (to look up or to approve) within `FAIL_WINDOW`.
const FAIL_MAX: usize = 5;
const FAIL_WINDOW: Duration = Duration::from_secs(600);
/// Polls per source address per minute (a real device needs 20).
const POLLS_PER_MIN: u32 = 120;
/// Tables of per-address counters are emptied beyond this size (an attacker with a million addresses must not fill the memory).
const TABLE_MAX: usize = 10_000;

/// What the per-address limits count by: an IPv4 address as it is, an IPv6 address as its /64 (one person or one attacker is given a
/// whole /64, so counting single addresses would give them 2^64 allowances). An IPv4-mapped IPv6 address counts as the IPv4 one.
pub fn addr_key(ip: &str) -> String {
    use std::net::IpAddr;
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => a.to_string(),
        Ok(IpAddr::V6(a)) => match a.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = a.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
        Err(_) => ip.to_string(),
    }
}

/// The caps. The defaults are the contract's; the tests use smaller ones.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub per_ip_per_min: u32,
    pub per_ip_pending: usize,
    pub global_pending: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { per_ip_per_min: 10, per_ip_pending: 5, global_pending: 1000 }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// Too many starts from this address: try again in this many seconds.
    Rate(u64),
    /// This address already has as many waiting requests as it may.
    TooManyHere,
    /// The whole server has as many as it can take.
    Full,
    NoRandom(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Started {
    pub device_code: String,
    pub user_code: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Polled {
    Pending,
    SlowDown,
    Approved { credential: String, email: String },
    Denied,
    /// Ran out, was collected already, or never existed (these are not told apart).
    Expired,
}

/// What the approval screen shows.
#[derive(Debug, PartialEq, Eq)]
pub struct Info {
    pub name: String,
    pub kind: Kind,
    pub requested_at: u64,
    pub ip: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CodeError {
    /// This person typed too many wrong codes: try again in this many seconds.
    Locked(u64),
    /// No such code (wrong, expired, or already used).
    Unknown,
}

/// A request that is being approved right now (see `begin_approve`).
#[derive(Debug, PartialEq, Eq)]
pub struct Claim {
    pub hash: String,
    pub name: String,
    pub kind: Kind,
}

enum State {
    Pending,
    /// Somebody is making the credential (the database is slow; the lock is not held meanwhile).
    Approving,
    /// The credential waits to be collected. `device` is its id in the person's list and `tenant` is whose.
    Approved { credential: String, email: String, tenant: String, device: String },
    Denied,
}

struct Req {
    name: String,
    kind: Kind,
    user_code: String,
    ip: String,
    /// `addr_key(ip)`: what the per-address limits count by.
    key: String,
    requested_at: u64,
    expires: Instant,
    last_poll: Option<Instant>,
    state: State,
}

#[derive(Default)]
struct Inner {
    /// sha256(device_code) -> request.
    by_hash: HashMap<String, Req>,
    /// user code -> sha256(device_code), for the requests that are waiting for approval.
    by_code: HashMap<String, String>,
    starts: HashMap<String, Bucket>,
    polls: HashMap<String, Bucket>,
    fails: HashMap<String, Vec<Instant>>,
    /// Credentials that were approved and never collected: (tenant, device id). The caller revokes them.
    orphans: Vec<(String, String)>,
}

pub struct DeviceLogin {
    inner: Mutex<Inner>,
    limits: Limits,
    /// How far the clock was moved (tests only; always 0 in the server).
    skew: AtomicU64,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "abcd efgh", "ABCD-EFGH" -> "ABCD-EFGH"; anything that cannot be a user code -> None.
pub fn normalize_user_code(s: &str) -> Option<String> {
    let c: String = s.chars().filter(|c| !c.is_whitespace() && *c != '-').map(|c| c.to_ascii_uppercase()).collect();
    (c.len() == 8 && c.bytes().all(|b| ALPHABET.contains(&b))).then(|| format!("{}-{}", &c[..4], &c[4..]))
}

fn user_code() -> Result<String, String> {
    // 256 is a multiple of 32, so taking the low five bits of a random byte is exactly uniform
    let b = random_bytes::<8>()?;
    let c: String = b.iter().map(|x| ALPHABET[(*x & 31) as usize] as char).collect();
    Ok(format!("{}-{}", &c[..4], &c[4..]))
}

/// A device code looks like what we make (43 url-safe characters): anything else is not looked up at all.
fn device_code_shaped(s: &str) -> bool {
    s.len() == 43 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl DeviceLogin {
    pub fn new(limits: Limits) -> DeviceLogin {
        DeviceLogin { inner: Mutex::new(Inner::default()), limits, skew: AtomicU64::new(0) }
    }

    /// The clock the requests are timed by.
    pub fn now(&self) -> Instant {
        Instant::now() + Duration::from_secs(self.skew.load(Ordering::SeqCst))
    }

    #[cfg(test)]
    pub fn advance(&self, secs: u64) {
        self.skew.fetch_add(secs, Ordering::SeqCst);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Drops what has run out. An approved credential nobody collected is queued to be revoked.
    fn sweep(i: &mut Inner, now: Instant) {
        let gone: Vec<String> = i.by_hash.iter().filter(|(_, r)| now >= r.expires).map(|(h, _)| h.clone()).collect();
        for h in gone {
            if let Some(r) = i.by_hash.remove(&h) {
                i.by_code.remove(&r.user_code);
                if let State::Approved { tenant, device, .. } = r.state {
                    i.orphans.push((tenant, device));
                }
            }
        }
        i.fails.retain(|_, v| {
            v.retain(|t| now.saturating_duration_since(*t) < FAIL_WINDOW);
            !v.is_empty()
        });
    }

    /// Housekeeping: call now and then (the server does, every half minute).
    pub fn sweep_now(&self, now: Instant) {
        Self::sweep(&mut self.lock(), now);
    }

    /// Credentials to revoke because their request ran out before the device collected them.
    pub fn take_orphans(&self) -> Vec<(String, String)> {
        std::mem::take(&mut self.lock().orphans)
    }

    pub fn pending(&self) -> usize {
        self.lock().by_hash.len()
    }

    /// A device asks to be signed in. `name` and `kind` were checked by the caller.
    pub fn start(&self, name: &str, kind: Kind, ip: &str, now: Instant) -> Result<Started, StartError> {
        let mut i = self.lock();
        Self::sweep(&mut i, now);
        if i.starts.len() > TABLE_MAX {
            i.starts.clear();
        }
        let per_min = self.limits.per_ip_per_min;
        let key = addr_key(ip);
        i.starts.entry(key.clone()).or_insert_with(|| Bucket::new(per_min, now)).take(now).map_err(StartError::Rate)?;
        let here = i.by_hash.values().filter(|r| r.key == key).count();
        if here >= self.limits.per_ip_pending {
            return Err(StartError::TooManyHere);
        }
        let all = i.by_hash.len();
        if all >= self.limits.global_pending {
            return Err(StartError::Full);
        }
        // the last fifth of the room is kept for networks that have nothing waiting, so a few busy (or hostile) networks cannot
        // take every place
        if here > 0 && all >= self.limits.global_pending * 4 / 5 {
            return Err(StartError::Full);
        }
        let device_code = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>().map_err(StartError::NoRandom)?);
        let mut code = user_code().map_err(StartError::NoRandom)?;
        for _ in 0..20 {
            if !i.by_code.contains_key(&code) {
                break;
            }
            code = user_code().map_err(StartError::NoRandom)?;
        }
        if i.by_code.contains_key(&code) {
            return Err(StartError::Full);
        }
        let hash = hash_token(&device_code);
        i.by_code.insert(code.clone(), hash.clone());
        i.by_hash.insert(hash, Req { name: name.to_string(), kind, user_code: code.clone(), ip: ip.to_string(), key: key.clone(), requested_at: unix_now(), expires: now + Duration::from_secs(EXPIRES_SECS), last_poll: None, state: State::Pending });
        Ok(Started { device_code, user_code: code })
    }

    /// The device asks whether the person approved. `Err(seconds)`: too many polls from this address.
    pub fn poll(&self, device_code: &str, ip: &str, now: Instant) -> Result<Polled, u64> {
        let mut i = self.lock();
        Self::sweep(&mut i, now);
        if i.polls.len() > TABLE_MAX {
            i.polls.clear();
        }
        i.polls.entry(addr_key(ip)).or_insert_with(|| Bucket::new(POLLS_PER_MIN, now)).take(now)?;
        if !device_code_shaped(device_code) {
            return Ok(Polled::Expired);
        }
        let hash = hash_token(device_code);
        let Some(r) = i.by_hash.get_mut(&hash) else { return Ok(Polled::Expired) };
        let too_fast = r.last_poll.is_some_and(|t| now.saturating_duration_since(t) < MIN_POLL_GAP);
        r.last_poll = Some(now);
        if too_fast {
            return Ok(Polled::SlowDown);
        }
        match r.state {
            State::Pending | State::Approving => Ok(Polled::Pending),
            State::Denied => {
                i.by_hash.remove(&hash);
                Ok(Polled::Denied)
            }
            State::Approved { .. } => {
                // the credential leaves memory with this answer, and the request is gone: a second poll finds nothing
                let Some(r) = i.by_hash.remove(&hash) else { return Ok(Polled::Expired) };
                match r.state {
                    State::Approved { credential, email, .. } => Ok(Polled::Approved { credential, email }),
                    _ => Ok(Polled::Expired),
                }
            }
        }
    }

    /// Wrong codes of this person within the window; refuses when they are used up.
    fn check_locked(i: &Inner, tenant: &str, now: Instant) -> Result<(), CodeError> {
        let Some(v) = i.fails.get(tenant) else { return Ok(()) };
        let recent: Vec<&Instant> = v.iter().filter(|t| now.saturating_duration_since(**t) < FAIL_WINDOW).collect();
        if recent.len() >= FAIL_MAX {
            let oldest = recent.iter().map(|t| now.saturating_duration_since(**t)).max().unwrap_or_default();
            return Err(CodeError::Locked(FAIL_WINDOW.saturating_sub(oldest).as_secs().max(1)));
        }
        Ok(())
    }

    fn fail(i: &mut Inner, tenant: &str, now: Instant) -> CodeError {
        if i.fails.len() > TABLE_MAX {
            i.fails.clear();
        }
        i.fails.entry(tenant.to_string()).or_default().push(now);
        CodeError::Unknown
    }

    /// Finds the waiting request for a code the person typed (the person's wrong tries are counted).
    fn find(i: &mut Inner, tenant: &str, code: &str, now: Instant) -> Result<String, CodeError> {
        Self::sweep(i, now);
        Self::check_locked(i, tenant, now)?;
        let hash = normalize_user_code(code).and_then(|c| i.by_code.get(&c).cloned());
        match hash.filter(|h| i.by_hash.get(h).is_some_and(|r| matches!(r.state, State::Pending))) {
            Some(h) => Ok(h),
            None => Err(Self::fail(i, tenant, now)),
        }
    }

    /// What the approval screen shows for a code.
    pub fn lookup(&self, tenant: &str, code: &str, now: Instant) -> Result<Info, CodeError> {
        let mut i = self.lock();
        let h = Self::find(&mut i, tenant, code, now)?;
        let r = &i.by_hash[&h];
        Ok(Info { name: r.name.clone(), kind: r.kind, requested_at: r.requested_at, ip: r.ip.clone() })
    }

    /// The person says no. The device will be told once.
    pub fn deny(&self, tenant: &str, code: &str, now: Instant) -> Result<(), CodeError> {
        let mut i = self.lock();
        let h = Self::find(&mut i, tenant, code, now)?;
        let r = i.by_hash.get_mut(&h).expect("found");
        r.state = State::Denied;
        let c = r.user_code.clone();
        i.by_code.remove(&c);
        Ok(())
    }

    /// The person says yes: takes the request (a second approval of the same code finds nothing) while the credential is made.
    /// Then `finish_approve`, or `abort_approve` if it could not be made.
    pub fn begin_approve(&self, tenant: &str, code: &str, now: Instant) -> Result<Claim, CodeError> {
        let mut i = self.lock();
        let h = Self::find(&mut i, tenant, code, now)?;
        let r = i.by_hash.get_mut(&h).expect("found");
        r.state = State::Approving;
        let (c, name, kind) = (r.user_code.clone(), r.name.clone(), r.kind);
        i.by_code.remove(&c);
        Ok(Claim { hash: h, name, kind })
    }

    /// Puts the made credential where the device's next poll collects it. False: the request ran out meanwhile (the caller must
    /// revoke the credential).
    pub fn finish_approve(&self, claim: &Claim, credential: String, email: String, tenant: &str, device: String, now: Instant) -> bool {
        let mut i = self.lock();
        Self::sweep(&mut i, now);
        match i.by_hash.get_mut(&claim.hash) {
            Some(r) => {
                r.state = State::Approved { credential, email, tenant: tenant.to_string(), device };
                true
            }
            None => false,
        }
    }

    /// The credential could not be made: the request waits for approval again.
    pub fn abort_approve(&self, claim: &Claim) {
        let mut i = self.lock();
        if let Some(r) = i.by_hash.get_mut(&claim.hash) {
            r.state = State::Pending;
            let c = r.user_code.clone();
            i.by_code.insert(c, claim.hash.clone());
        }
    }
}

/// The server's own public address, as people and devices are told it: `https://host[:port]`, nothing else. The approval page is
/// `<this>/device`. Anything with a path, a query, a fragment, a user name, a scheme other than https, or odd characters is refused,
/// so the address a device shows to a person cannot be bent to point somewhere else.
pub fn parse_public_base(s: &str) -> Result<String, String> {
    let bad = |why: &str| Err(format!("{why} (it must look like https://zo-ai.example.com: https, a host name, no path)"));
    let s = s.trim();
    let Some(rest) = s.strip_prefix("https://") else { return bad("the public address must start with https://") };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = match rest.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (rest, None),
    };
    if host.is_empty() || host.len() > 253 || !host.contains('.') && host != "localhost" {
        return bad("the public address has no proper host name");
    }
    if !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') || host.starts_with(['-', '.']) || host.ends_with(['-', '.']) || host.contains("..") {
        return bad("the public address has characters a host name cannot have, or a path, query or user name");
    }
    if let Some(p) = port {
        if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) || p.parse::<u32>().map_or(true, |n| n == 0 || n > 65535) {
            return bad("the public address has a bad port");
        }
    }
    Ok(format!("https://{}{}", host.to_ascii_lowercase(), port.map(|p| format!(":{p}")).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl() -> DeviceLogin {
        DeviceLogin::new(Limits::default())
    }

    #[test]
    fn codes_have_the_documented_shape() {
        let d = dl();
        let s = d.start("Laptop", Kind::Desktop, "1.1.1.1", d.now()).unwrap();
        assert_eq!(s.device_code.len(), 43);
        assert!(device_code_shaped(&s.device_code));
        assert_eq!(s.user_code.len(), 9);
        assert_eq!(&s.user_code[4..5], "-");
        assert!(s.user_code.replace('-', "").bytes().all(|b| ALPHABET.contains(&b)));
        assert!(!s.user_code.contains(['0', 'O', '1', 'I', 'L']));
        let t = d.start("Other", Kind::Cli, "1.1.1.1", d.now()).unwrap();
        assert_ne!((&s.device_code, &s.user_code), (&t.device_code, &t.user_code));
        // typing it as people do: lower case, a space instead of the dash, no dash
        assert_eq!(normalize_user_code(&s.user_code.to_lowercase().replace('-', " ")).as_deref(), Some(s.user_code.as_str()));
        assert_eq!(normalize_user_code(&s.user_code.replace('-', "")).as_deref(), Some(s.user_code.as_str()));
        for bad in ["", "ABCD-EFG", "ABCD-EFGHJ", "ABCD-EFG0", "ABCD-EFGI", "ABCD-EF!H"] {
            assert_eq!(normalize_user_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_device_code_itself_is_never_stored() {
        let d = dl();
        let s = d.start("Laptop", Kind::Desktop, "1.1.1.1", d.now()).unwrap();
        let i = d.lock();
        assert!(i.by_hash.keys().all(|k| *k != s.device_code && k.len() == 64));
        assert!(i.by_code.values().all(|v| *v != s.device_code));
    }

    #[test]
    fn approve_poll_once() {
        let d = dl();
        let (t, now) = ("tenant1", d.now());
        let s = d.start("My laptop", Kind::Cli, "9.9.9.9", now).unwrap();
        assert_eq!(d.poll(&s.device_code, "9.9.9.9", now), Ok(Polled::Pending));
        let info = d.lookup(t, &s.user_code, now).unwrap();
        assert_eq!((info.name.as_str(), info.kind, info.ip.as_str()), ("My laptop", Kind::Cli, "9.9.9.9"));
        let claim = d.begin_approve(t, &s.user_code, now).unwrap();
        // while the credential is being made, the code is taken: nobody can approve (or deny) it twice
        assert_eq!(d.begin_approve("tenant2", &s.user_code, now), Err(CodeError::Unknown));
        assert_eq!(d.poll(&s.device_code, "9.9.9.9", now + Duration::from_secs(3)), Ok(Polled::Pending));
        assert!(d.finish_approve(&claim, "zd_x".into(), "a@b.c".into(), t, "dev1".into(), now));
        let later = now + Duration::from_secs(6);
        assert_eq!(d.poll(&s.device_code, "9.9.9.9", later), Ok(Polled::Approved { credential: "zd_x".into(), email: "a@b.c".into() }));
        assert_eq!(d.poll(&s.device_code, "9.9.9.9", later + Duration::from_secs(3)), Ok(Polled::Expired), "handed out once");
        assert_eq!(d.pending(), 0);
        assert!(d.take_orphans().is_empty());
        // the code is single use
        assert_eq!(d.lookup(t, &s.user_code, later), Err(CodeError::Unknown));
    }

    #[test]
    fn a_failed_credential_puts_the_request_back() {
        let d = dl();
        let now = d.now();
        let s = d.start("x", Kind::Desktop, "1.1.1.1", now).unwrap();
        let claim = d.begin_approve("t", &s.user_code, now).unwrap();
        d.abort_approve(&claim);
        assert!(d.lookup("t", &s.user_code, now).is_ok());
    }

    #[test]
    fn deny_tells_the_device_once() {
        let d = dl();
        let now = d.now();
        let s = d.start("x", Kind::Desktop, "1.1.1.1", now).unwrap();
        d.deny("t", &s.user_code, now).unwrap();
        assert_eq!(d.deny("t", &s.user_code, now), Err(CodeError::Unknown));
        assert_eq!(d.poll(&s.device_code, "1.1.1.1", now), Ok(Polled::Denied));
        assert_eq!(d.poll(&s.device_code, "1.1.1.1", now + Duration::from_secs(3)), Ok(Polled::Expired));
    }

    #[test]
    fn requests_run_out_after_ten_minutes_and_an_uncollected_credential_is_queued_for_revoking() {
        let d = dl();
        let now = d.now();
        let a = d.start("a", Kind::Desktop, "1.1.1.1", now).unwrap();
        let b = d.start("b", Kind::Desktop, "1.1.1.1", now).unwrap();
        let claim = d.begin_approve("t", &b.user_code, now).unwrap();
        assert!(d.finish_approve(&claim, "zd_b".into(), "e".into(), "t", "dev-b".into(), now));
        let just_before = now + Duration::from_secs(EXPIRES_SECS - 1);
        assert_eq!(d.poll(&a.device_code, "1.1.1.1", just_before), Ok(Polled::Pending));
        let after = now + Duration::from_secs(EXPIRES_SECS);
        assert_eq!(d.lookup("t", &a.user_code, after), Err(CodeError::Unknown));
        assert_eq!(d.poll(&a.device_code, "1.1.1.1", after), Ok(Polled::Expired));
        assert_eq!(d.poll(&b.device_code, "1.1.1.1", after), Ok(Polled::Expired));
        assert_eq!(d.pending(), 0);
        assert_eq!(d.take_orphans(), vec![("t".to_string(), "dev-b".to_string())]);
        assert!(d.take_orphans().is_empty());
        // approval that finishes after the request ran out is refused (the caller revokes what it made)
        let c = d.start("c", Kind::Desktop, "1.1.1.1", after).unwrap();
        let claim = d.begin_approve("t", &c.user_code, after).unwrap();
        assert!(!d.finish_approve(&claim, "zd_c".into(), "e".into(), "t", "dev-c".into(), after + Duration::from_secs(EXPIRES_SECS)));
    }

    #[test]
    fn polling_faster_than_the_interval_is_told_to_slow_down() {
        let d = dl();
        let now = d.now();
        let s = d.start("x", Kind::Desktop, "1.1.1.1", now).unwrap();
        assert_eq!(d.poll(&s.device_code, "1.1.1.1", now), Ok(Polled::Pending));
        assert_eq!(d.poll(&s.device_code, "1.1.1.1", now + Duration::from_secs(1)), Ok(Polled::SlowDown));
        // asking too early restarts the wait
        assert_eq!(d.poll(&s.device_code, "1.1.1.1", now + Duration::from_secs(3)), Ok(Polled::SlowDown));
        assert_eq!(d.poll(&s.device_code, "1.1.1.1", now + Duration::from_secs(6)), Ok(Polled::Pending));
        // an unknown or malformed code is simply expired
        assert_eq!(d.poll(&"A".repeat(43), "1.1.1.1", now + Duration::from_secs(9)), Ok(Polled::Expired));
        assert_eq!(d.poll("short", "1.1.1.1", now + Duration::from_secs(9)), Ok(Polled::Expired));
        assert_eq!(d.poll("", "1.1.1.1", now + Duration::from_secs(9)), Ok(Polled::Expired));
    }

    #[test]
    fn caps_per_address_per_minute_pending_per_address_and_pending_overall() {
        // five waiting per address, and another address is not affected
        let d = dl();
        let now = d.now();
        for _ in 0..5 {
            d.start("x", Kind::Desktop, "1.1.1.1", now).unwrap();
        }
        assert_eq!(d.start("x", Kind::Desktop, "1.1.1.1", now), Err(StartError::TooManyHere));
        assert!(d.start("x", Kind::Desktop, "2.2.2.2", now).is_ok());
        // a finished one frees a place
        let s = d.start("y", Kind::Desktop, "3.3.3.3", now).unwrap();
        d.deny("t", &s.user_code, now).unwrap();
        d.poll(&s.device_code, "3.3.3.3", now).unwrap();
        // ten starts a minute per address (pending cap out of the way here)
        let d = DeviceLogin::new(Limits { per_ip_pending: 1000, ..Limits::default() });
        let now = d.now();
        for n in 0..10 {
            d.start("x", Kind::Desktop, "1.1.1.1", now).unwrap_or_else(|e| panic!("{n}: {e:?}"));
        }
        assert!(matches!(d.start("x", Kind::Desktop, "1.1.1.1", now), Err(StartError::Rate(s)) if s >= 1));
        assert!(d.start("x", Kind::Desktop, "2.2.2.2", now).is_ok());
        assert!(d.start("x", Kind::Desktop, "1.1.1.1", now + Duration::from_secs(61)).is_ok(), "the allowance comes back");
        // a thousand waiting on the whole server
        let d = dl();
        let now = d.now();
        for n in 0..1000 {
            d.start("x", Kind::Desktop, &format!("10.0.{}.{}", n / 200, n % 200), now).unwrap_or_else(|e| panic!("{n}: {e:?}"));
        }
        assert_eq!(d.pending(), 1000);
        assert_eq!(d.start("x", Kind::Desktop, "9.9.9.9", now), Err(StartError::Full));
        // and when they run out the room is back
        assert!(d.start("x", Kind::Desktop, "9.9.9.9", now + Duration::from_secs(EXPIRES_SECS)).is_ok());
    }

    #[test]
    fn ipv6_counts_by_its_64_and_busy_networks_leave_room() {
        assert_eq!(addr_key("2001:db8:1:2:aaaa::1"), addr_key("2001:db8:1:2:bbbb::9"));
        assert_ne!(addr_key("2001:db8:1:2::1"), addr_key("2001:db8:1:3::1"));
        assert_eq!(addr_key("::ffff:1.2.3.4"), "1.2.3.4");
        // one /64 gets one allowance, however many addresses it uses
        let d = dl();
        let now = d.now();
        for n in 0..5 {
            d.start("x", Kind::Desktop, &format!("2001:db8:1:2::{n:x}"), now).unwrap();
        }
        assert_eq!(d.start("x", Kind::Desktop, "2001:db8:1:2::ff", now), Err(StartError::TooManyHere));
        // the last fifth of the room is for networks with nothing waiting
        let d = DeviceLogin::new(Limits { global_pending: 10, per_ip_pending: 100, per_ip_per_min: 1000 });
        let now = d.now();
        for n in 0..8 {
            d.start("x", Kind::Desktop, "1.1.1.1", now).unwrap_or_else(|e| panic!("{n}: {e:?}"));
        }
        assert_eq!(d.start("x", Kind::Desktop, "1.1.1.1", now), Err(StartError::Full), "a network that already waits cannot take the reserve");
        assert!(d.start("x", Kind::Desktop, "2.2.2.2", now).is_ok(), "a new network can");
    }

    #[test]
    fn five_wrong_codes_in_ten_minutes_lock_that_person_only() {
        let d = dl();
        let now = d.now();
        let s = d.start("x", Kind::Desktop, "1.1.1.1", now).unwrap();
        for n in 0..5 {
            assert_eq!(d.lookup("guesser", "AAAA-AAAA", now), Err(CodeError::Unknown), "{n}");
        }
        // now even the right code is refused for them
        assert!(matches!(d.lookup("guesser", &s.user_code, now), Err(CodeError::Locked(w)) if w >= 1 && w <= 600));
        assert!(matches!(d.begin_approve("guesser", &s.user_code, now), Err(CodeError::Locked(_))));
        assert!(matches!(d.deny("guesser", &s.user_code, now), Err(CodeError::Locked(_))));
        // somebody else is not locked, and finds the request still waiting
        assert!(d.lookup("honest", &s.user_code, now).is_ok());
        // right codes do not count against anybody
        for _ in 0..20 {
            assert!(d.lookup("honest", &s.user_code, now).is_ok());
        }
        // the lock ends
        let later = now + Duration::from_secs(601);
        let fresh = d.start("x", Kind::Desktop, "2.2.2.2", later).unwrap();
        assert!(d.lookup("guesser", &fresh.user_code, later).is_ok());
        // a malformed code and the code of an already used request count too
        let d = dl();
        let now = d.now();
        for bad in ["", "x", "ABCD", "ABCD-EFG0", "ABCD-EFGH"] {
            let _ = d.lookup("g", bad, now);
        }
        assert!(matches!(d.lookup("g", "ABCD-EFGH", now), Err(CodeError::Locked(_))));
    }

    #[test]
    fn public_base_addresses_are_checked() {
        assert_eq!(parse_public_base("https://zo-ai-server.onrender.com"), Ok("https://zo-ai-server.onrender.com".into()));
        assert_eq!(parse_public_base(" https://Zo-AI.example.com/ "), Ok("https://zo-ai.example.com".into()));
        assert_eq!(parse_public_base("https://zo.example.com:8443"), Ok("https://zo.example.com:8443".into()));
        for bad in [
            "",
            "zo.example.com",
            "http://zo.example.com",
            "https://",
            "https://zo",
            "https://zo.example.com/app",
            "https://zo.example.com//",
            "https://zo.example.com?x=1",
            "https://zo.example.com#f",
            "https://user@zo.example.com",
            "https://zo.example.com@evil.test",
            "https://zo.example.com:0",
            "https://zo.example.com:99999",
            "https://zo.example.com:",
            "https://zo.example.com:80a",
            "https://zo .example.com",
            "https://.example.com",
            "https://exa..mple.com",
            "https://[::1]",
            "javascript:alert(1)",
        ] {
            assert!(parse_public_base(bad).is_err(), "{bad:?}");
        }
    }
}
