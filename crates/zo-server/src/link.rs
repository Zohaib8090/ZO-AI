//! The desktop link: the web page runs ZO-AI on the person's OWN computer. The desktop app (signed in, device credential of kind
//! `desktop`) keeps an outbound long-poll open to the server (`/api/link/poll`); the web page sends its message to the server
//! (`/api/send` with `"target": "desktop"`), the server hands it to the waiting desktop as a job, the desktop runs it with its own
//! tools and posts the events back (`/api/link/events`), and the server relays them to the web page as the usual NDJSON stream.
//! The protocol, the security model and the limits are written down in docs/LINK.md.
//!
//! Everything here is per person: a device and a job belong to one tenant, and no call can name another tenant's device or job.
//! What a desktop posts is untrusted input (it only ever reaches the same person's own web page, but it is size-checked and
//! filtered all the same); what the web page sends is untrusted for the desktop, so only whitelisted fields travel on.
//! Nothing is stored: presence, queues and jobs live in memory and are gone at a restart (a job is never resumed).

use crate::devices::{clean_name, random_bytes, strip_name};
use crate::tenants::Bucket;
use crate::{App, Fail, Who};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tiny_http::{Request, Response};

/// How long a poll waits for a job before it answers with an empty list.
pub const POLL_WAIT: Duration = Duration::from_secs(25);
/// A desktop is online when it polled within this time (a poll that is open counts as online).
pub const ONLINE_WINDOW: Duration = Duration::from_secs(40);
/// A job nobody picked up within this time expires. It is never delivered later, never run late.
pub const JOB_EXPIRY: Duration = Duration::from_secs(120);
/// The longest a job may run.
pub const JOB_MAX: Duration = Duration::from_secs(60 * 60);
/// A delivered job whose desktop has been silent (no poll at all) this long is given up.
pub const LOST_AFTER: Duration = Duration::from_secs(120);
/// How long the web page waits for the answer of a whitelisted settings question.
pub const OP_WAIT: Duration = Duration::from_secs(45);
/// After Stop, how long the desktop has to end the job before the page is told it stopped anyway.
pub const STOP_GRACE: Duration = Duration::from_secs(20);
/// A silent stream gets a `ping` line this often (proxies close idle connections).
pub const PING_EVERY: Duration = Duration::from_secs(15);

/// Per person: devices that may be linked, and jobs that may run at once.
pub const MAX_DEVICES_PER_TENANT: usize = 10;
pub const MAX_SENDS_PER_TENANT: usize = 3;
/// Per device: jobs waiting in the queue, settings questions at a time.
pub const MAX_QUEUED_PER_DEVICE: usize = 8;
pub const MAX_OPS_PER_DEVICE: usize = 4;
/// For the whole server.
pub const MAX_DEVICES_ALL: usize = 5000;
pub const MAX_JOBS_ALL: usize = 400;
pub const MAX_POLLS_ALL: usize = 150;
/// Polls per minute and event posts per minute from one device.
pub const POLLS_PER_MIN: u32 = 20;
pub const EVENT_POSTS_PER_MIN: u32 = 600;
/// What one event post may hold: bytes (the request body limit), events.
pub const POLL_BODY_LIMIT: usize = 16 * 1024;
pub const EVENTS_PER_POST: usize = 500;
/// All events of one job; events waiting to be written to a web page that reads slowly.
pub const JOB_BYTES: usize = 50 * 1024 * 1024;
pub const PENDING_BYTES: usize = 8 * 1024 * 1024;
/// A message the web page may send to a computer.
pub const MAX_TEXT_CHARS: usize = 100_000;

/// The settings questions the web page may ask a desktop (everything else is refused here, and again on the desktop).
/// The last four are the read-only control operations of docs/FULL-CONTROL.md phase 1 (the desktop asks its person before the first
/// three of them run); they take one argument, a path.
pub const OPS: [&str; 11] = ["status", "providers", "profiles", "profile_use", "models", "select", "mode", "sys_info", "fs_list", "fs_read", "screenshot"];
/// The control operations wait for a person to answer a dialog on their computer, so the page's question waits longer for them.
pub fn is_control(op: &str) -> bool {
    matches!(op, "sys_info" | "fs_list" | "fs_read" | "screenshot")
}
pub const MAX_PATH_CHARS: usize = 1024;

pub const OFFLINE_MESSAGE: &str = "Your computer is not reachable. Start ZO-AI on your computer (it can run in the tray) and turn on Settings > Account and sync > Control this computer from the web.";

#[derive(Clone, Debug)]
pub struct Config {
    pub poll_wait: Duration,
    pub online_window: Duration,
    pub job_expiry: Duration,
    pub job_max: Duration,
    pub lost_after: Duration,
    pub op_wait: Duration,
    pub stop_grace: Duration,
    pub ping_every: Duration,
    /// How often a waiting relay looks at the clock.
    pub tick: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config { poll_wait: POLL_WAIT, online_window: ONLINE_WINDOW, job_expiry: JOB_EXPIRY, job_max: JOB_MAX, lost_after: LOST_AFTER, op_wait: OP_WAIT, stop_grace: STOP_GRACE, ping_every: PING_EVERY, tick: Duration::from_secs(1) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Send,
    Op,
}

/// One thing waiting for a desktop's next poll: a job, or a control message (stop, an approval answer) about a job.
struct QItem {
    /// The job this delivers (it is then "picked up"); None for a control message.
    job: Option<String>,
    payload: Value,
    at: Instant,
}

struct Dev {
    name: String,
    caps: Value,
    approvals: bool,
    last_seen: Instant,
    last_seen_unix: u64,
    poll_gen: u64,
    in_poll: bool,
    queue: VecDeque<QItem>,
    poll_rate: Bucket,
    event_rate: Bucket,
}

struct Job {
    tenant: String,
    device: String,
    kind: Kind,
    created: Instant,
    delivered: bool,
    last_activity: Instant,
    events: VecDeque<Value>,
    pending: usize,
    total: usize,
    done: bool,
    cancel: bool,
    cancel_at: Option<Instant>,
    /// Why it ended without the desktop saying so: (code, message).
    fail: Option<(&'static str, String)>,
    result: Option<Value>,
}

#[derive(Default)]
struct Inner {
    devs: HashMap<(String, String), Dev>,
    jobs: HashMap<String, Job>,
    streams: HashMap<u64, String>,
    /// Jobs that are over, remembered a while so that a desktop that still posts for one is told to stop.
    ended: HashMap<String, (String, String, Instant)>,
    polls: usize,
}

/// What the page reads next from a running job.
#[derive(Debug)]
pub enum Relay {
    Events(Vec<Value>),
    /// Nothing for a while.
    Idle,
    /// Over: the last events, and why if the desktop did not end it itself.
    End(Vec<Value>, Option<(&'static str, String)>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendErr {
    /// No online desktop (or not that one).
    Offline,
    /// This computer is running a web request already.
    Busy,
    /// One of the per-person or server-wide limits.
    Limit(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Replied {
    Sent,
    NotFound,
    /// The desktop does not take approval answers from the web.
    ApprovalsLocal,
    Bad(&'static str),
}

pub struct Link {
    pub cfg: Config,
    inner: Mutex<Inner>,
    pollers: Condvar,
    relays: Condvar,
}

impl Default for Link {
    fn default() -> Link {
        Link::new(Config::default())
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn new_id() -> String {
    random_bytes::<16>().map(|b| b.iter().map(|x| format!("{x:02x}")).collect()).unwrap_or_else(|_| format!("{:032x}", now_unix() as u128 * 7919 + std::time::Instant::now().elapsed().as_nanos()))
}

fn short(v: &Value, k: &str, max: usize) -> String {
    strip_name(v.get(k).and_then(Value::as_str).unwrap_or("")).chars().take(max).collect()
}

/// What a desktop says about itself, reduced to the fields that are known, with their types and sizes checked.
fn clean_caps(v: &Value) -> (Value, bool) {
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    let approvals = b("approvals_from_web");
    (json!({"claude_code": b("claude_code"), "approvals_from_web": approvals, "version": short(v, "version", 40), "os": short(v, "os", 40)}), approvals)
}

impl Link {
    pub fn new(cfg: Config) -> Link {
        Link { cfg, inner: Mutex::new(Inner::default()), pollers: Condvar::new(), relays: Condvar::new() }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn online(&self, d: &Dev, now: Instant) -> bool {
        d.in_poll || now.saturating_duration_since(d.last_seen) < self.cfg.online_window
    }

    // -------------------------------------------------------------------------------------------------------------------
    // the desktop's side

    /// A desktop asks for work: waits up to `poll_wait` for something, then answers with what is waiting (maybe nothing). One
    /// poll per device is open at a time: a newer one ends an older one at once.
    pub fn poll(&self, tenant: &str, dev: &str, name: &str, caps: &Value) -> Result<Vec<Value>, Fail> {
        let key = (tenant.to_string(), dev.to_string());
        let (caps, approvals) = clean_caps(caps);
        let name = clean_name(name);
        let mut g = self.lock();
        let now = Instant::now();
        if g.polls >= MAX_POLLS_ALL {
            return Err(Fail { status: 503, error: "The server has as many computers waiting as it can take. Try again in a moment.".into(), retry: Some(10) });
        }
        if !g.devs.contains_key(&key) {
            if g.devs.keys().filter(|(t, _)| t == tenant).count() >= MAX_DEVICES_PER_TENANT {
                return Err(Fail::new(409, "Too many computers are linked to this account."));
            }
            if g.devs.len() >= MAX_DEVICES_ALL {
                return Err(Fail { status: 503, error: "The server has as many linked computers as it can take.".into(), retry: Some(30) });
            }
            g.devs.insert(key.clone(), Dev { name: String::new(), caps: json!({}), approvals: false, last_seen: now, last_seen_unix: now_unix(), poll_gen: 0, in_poll: false, queue: VecDeque::new(), poll_rate: Bucket::new(POLLS_PER_MIN, now), event_rate: Bucket::new(EVENT_POSTS_PER_MIN, now) });
        }
        let my = {
            let d = g.devs.get_mut(&key).expect("just made");
            d.poll_rate.take(now).map_err(|w| Fail { status: 429, error: "This computer is polling too fast.".into(), retry: Some(w) })?;
            if !name.is_empty() {
                d.name = name;
            }
            d.caps = caps;
            d.approvals = approvals;
            d.poll_gen += 1;
            d.in_poll = true;
            d.last_seen = now;
            d.last_seen_unix = now_unix();
            d.poll_gen
        };
        g.polls += 1;
        self.pollers.notify_all(); // an older poll of this device ends
        let deadline = now + self.cfg.poll_wait;
        let out = loop {
            let now = Instant::now();
            let Some(d) = g.devs.get(&key) else { break Vec::new() };
            if d.poll_gen != my {
                break Vec::new();
            }
            let jobs = self.deliver(&mut g, &key, now);
            if !jobs.is_empty() || now >= deadline {
                break jobs;
            }
            g = self.pollers.wait_timeout(g, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        };
        g.polls = g.polls.saturating_sub(1);
        if let Some(d) = g.devs.get_mut(&key) {
            if d.poll_gen == my {
                d.in_poll = false;
                d.last_seen = Instant::now();
                d.last_seen_unix = now_unix();
            }
        }
        Ok(out)
    }

    /// Takes what waits for one device. Things that waited too long are not delivered: a job that expired is over.
    fn deliver(&self, g: &mut Inner, key: &(String, String), now: Instant) -> Vec<Value> {
        let items: Vec<QItem> = match g.devs.get_mut(key) {
            Some(d) => d.queue.drain(..).collect(),
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut changed = false;
        for it in items {
            let age = now.saturating_duration_since(it.at);
            if age >= self.cfg.job_expiry {
                if let Some(j) = it.job.as_ref().and_then(|id| g.jobs.get_mut(id)) {
                    if !j.done {
                        j.fail = Some(("desktop_timeout", "Your computer did not pick up the request in time, so it was not run. Try again.".into()));
                        j.done = true;
                        changed = true;
                    }
                }
                continue;
            }
            if let Some(id) = &it.job {
                match g.jobs.get_mut(id) {
                    Some(j) if !j.done && !j.cancel => {
                        j.delivered = true;
                        j.last_activity = now;
                    }
                    _ => continue, // cancelled or gone before it was picked up: never delivered
                }
            }
            let mut p = it.payload;
            p["age_secs"] = json!(age.as_secs());
            p["age_ms"] = json!(age.as_millis() as u64);
            p["expires_in_secs"] = json!(self.cfg.job_expiry.saturating_sub(age).as_secs());
            out.push(p);
        }
        if changed {
            self.relays.notify_all();
        }
        out
    }

    /// A desktop posts events of one of its jobs. Returns whether it must stop (the page pressed Stop or went away, a limit was
    /// hit, the job is over). `Err(None)`: not a job of this device.
    pub fn events(&self, tenant: &str, dev: &str, job: &str, events: &[Value], done: bool) -> Result<bool, Fail> {
        let key = (tenant.to_string(), dev.to_string());
        let mut g = self.lock();
        let now = Instant::now();
        let inner = &mut *g;
        // a job that is over is known for a while: the desktop is told to stop instead of an error
        if !inner.jobs.contains_key(job) {
            return match inner.ended.get(job) {
                Some((t, d, _)) if *t == tenant && *d == dev => Ok(true),
                _ => Err(Fail::new(404, "unknown_job")),
            };
        }
        let j = inner.jobs.get_mut(job).expect("checked");
        if j.tenant != tenant || j.device != dev {
            return Err(Fail::new(404, "unknown_job"));
        }
        let d = inner.devs.get_mut(&key).ok_or_else(|| Fail::new(404, "unknown_job"))?;
        d.event_rate.take(now).map_err(|w| Fail { status: 429, error: "This computer is posting too fast.".into(), retry: Some(w) })?;
        d.last_seen = now;
        d.last_seen_unix = now_unix();
        if j.done {
            return Ok(true);
        }
        j.delivered = true;
        j.last_activity = now;
        let size: usize = events.iter().map(|e| e.to_string().len()).sum();
        if j.total + size > JOB_BYTES || j.pending + size > PENDING_BYTES {
            j.fail = Some(("too_much_output", "The job sent more than the server relays, so it was stopped.".into()));
            j.cancel = true;
            j.done = true;
            self.relays.notify_all();
            return Ok(true);
        }
        j.total += size;
        match j.kind {
            Kind::Send => {
                for e in events {
                    // the server writes the first and the last line of a stream itself
                    if matches!(e.get("event").and_then(Value::as_str), Some("stream" | "done")) {
                        continue;
                    }
                    j.pending += e.to_string().len();
                    j.events.push_back(e.clone());
                }
            }
            Kind::Op => {
                // the desktop answers with `{"event": "op_result", "reply": {...}}` (or `result`), or with an error event
                for e in events {
                    match e.get("event").and_then(Value::as_str) {
                        Some("op_result" | "result") if j.result.is_none() => j.result = Some(e.get("reply").or_else(|| e.get("result")).cloned().unwrap_or(Value::Null)),
                        Some("error") if j.fail.is_none() => {
                            let m: String = e.get("error").or_else(|| e.get("message")).and_then(Value::as_str).unwrap_or("Your computer could not answer.").chars().take(300).collect();
                            j.fail = Some(("desktop_error", strip_name(&m)));
                        }
                        _ => {}
                    }
                }
            }
        }
        if done {
            j.done = true;
        }
        let cancel = j.cancel || j.fail.is_some();
        self.relays.notify_all();
        Ok(cancel)
    }

    // -------------------------------------------------------------------------------------------------------------------
    // the web page's side

    /// Starts a message on one of the person's computers: `want` is the device the page chose (else the one seen most recently
    /// that is online). Returns the job and the device's name. `payload` is the job as the desktop gets it (without `id`/`type`).
    pub fn submit_send(&self, tenant: &str, want: Option<&str>, payload: Value, stream: u64) -> Result<(String, String), SendErr> {
        let mut g = self.lock();
        let now = Instant::now();
        let dev = self.pick(&g, tenant, want, now).ok_or(SendErr::Offline)?;
        let inner = &mut *g;
        if inner.jobs.values().any(|j| j.kind == Kind::Send && j.device == dev && j.tenant == tenant && !j.done && j.fail.is_none()) {
            return Err(SendErr::Busy);
        }
        if inner.jobs.values().filter(|j| j.tenant == tenant && j.kind == Kind::Send && !j.done && j.fail.is_none()).count() >= MAX_SENDS_PER_TENANT {
            return Err(SendErr::Limit("You have too many web requests running on your computers. Wait for one to finish."));
        }
        if inner.jobs.len() >= MAX_JOBS_ALL {
            return Err(SendErr::Limit("The server is relaying as many requests as it can right now. Try again in a moment."));
        }
        let id = new_id();
        let d = inner.devs.get_mut(&(tenant.to_string(), dev.clone())).expect("picked");
        if d.queue.len() >= MAX_QUEUED_PER_DEVICE {
            return Err(SendErr::Limit("Too many requests are waiting for this computer."));
        }
        let name = d.name.clone();
        let mut p = payload;
        p["id"] = json!(id);
        p["type"] = json!("send");
        d.queue.push_back(QItem { job: Some(id.clone()), payload: p, at: now });
        inner.jobs.insert(id.clone(), Job { tenant: tenant.into(), device: dev, kind: Kind::Send, created: now, delivered: false, last_activity: now, events: VecDeque::new(), pending: 0, total: 0, done: false, cancel: false, cancel_at: None, fail: None, result: None });
        inner.streams.insert(stream, id.clone());
        self.pollers.notify_all();
        Ok((id, name))
    }

    /// The online device to use: the chosen one, or the most recently seen. Only of this person.
    fn pick(&self, g: &Inner, tenant: &str, want: Option<&str>, now: Instant) -> Option<String> {
        g.devs
            .iter()
            .filter(|((t, id), d)| t == tenant && self.online(d, now) && want.is_none_or(|w| w == id))
            .max_by_key(|(_, d)| d.last_seen)
            .map(|((_, id), _)| id.clone())
    }

    /// Reads what a running job has produced, waiting at most `max` for something.
    pub fn relay_wait(&self, job: &str, max: Duration) -> Relay {
        let mut g = self.lock();
        let end = Instant::now() + max;
        loop {
            let now = Instant::now();
            let inner = &mut *g;
            let lost_after = self.cfg.lost_after;
            let seen = inner.jobs.get(job).and_then(|j| inner.devs.get(&(j.tenant.clone(), j.device.clone()))).map(|d| if d.in_poll { now } else { d.last_seen });
            let Some(j) = inner.jobs.get_mut(job) else { return Relay::End(Vec::new(), Some(("closed", "This request is no longer running.".into()))) };
            if !j.done {
                if !j.delivered && now.saturating_duration_since(j.created) >= self.cfg.job_expiry {
                    j.fail = Some(("desktop_timeout", "Your computer did not pick up the request in time, so it was not run. Try again.".into()));
                    j.done = true;
                } else if j.delivered && seen.is_none_or(|s| now.saturating_duration_since(s) >= lost_after) {
                    j.fail = Some(("desktop_lost", "The connection to your computer was lost. The task may still be running there: check Running now on your computer.".into()));
                    j.cancel = true;
                    j.done = true;
                } else if now.saturating_duration_since(j.created) >= self.cfg.job_max {
                    j.fail = Some(("job_timeout", "The request ran for an hour and was stopped.".into()));
                    j.cancel = true;
                    j.done = true;
                } else if j.cancel && j.cancel_at.is_some_and(|c| now.saturating_duration_since(c) >= self.cfg.stop_grace) {
                    j.fail = Some(("stopped", "Stopped.".into()));
                    j.done = true;
                }
            }
            let evs: Vec<Value> = j.events.drain(..).collect();
            j.pending = 0;
            if j.done {
                return Relay::End(evs, j.fail.clone());
            }
            if !evs.is_empty() {
                return Relay::Events(evs);
            }
            if now >= end {
                return Relay::Idle;
            }
            let slice = (end - now).min(self.cfg.tick);
            g = self.relays.wait_timeout(g, slice).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// The page closes a job (it ended, or the page went away): it is forgotten, a desktop that is still running it is told to stop.
    pub fn finish(&self, job: &str, stream: u64) {
        let mut g = self.lock();
        let inner = &mut *g;
        inner.streams.remove(&stream);
        let Some(j) = inner.jobs.remove(job) else { return };
        let key = (j.tenant.clone(), j.device.clone());
        if let Some(d) = inner.devs.get_mut(&key) {
            d.queue.retain(|q| q.job.as_deref() != Some(job));
            if j.delivered && !j.done {
                d.queue.push_back(QItem { job: None, payload: json!({"id": new_id(), "type": "stop", "job_id": job}), at: Instant::now() });
                self.pollers.notify_all();
            }
        }
        if inner.ended.len() > 10_000 {
            inner.ended.clear();
        }
        inner.ended.insert(job.to_string(), (j.tenant, j.device, Instant::now()));
    }

    /// The page went away: the desktop is told to stop (by the next events answer, and by `finish`).
    pub fn cancel_job(&self, job: &str) {
        let mut g = self.lock();
        if let Some(j) = g.jobs.get_mut(job) {
            if !j.done {
                j.cancel = true;
                j.cancel_at.get_or_insert(Instant::now());
            }
        }
    }

    /// A line from the page for a running job (`/api/reply`): Stop, or the answer to an approval.
    pub fn reply(&self, tenant: &str, stream: u64, v: &Value) -> Replied {
        let mut g = self.lock();
        let inner = &mut *g;
        let Some(job) = inner.streams.get(&stream).cloned() else { return Replied::NotFound };
        let Some(j) = inner.jobs.get_mut(&job) else { return Replied::NotFound };
        if j.tenant != tenant {
            return Replied::NotFound;
        }
        let key = (j.tenant.clone(), j.device.clone());
        match v.get("op").and_then(Value::as_str) {
            Some("stop") => {
                if !j.cancel {
                    j.cancel = true;
                    j.cancel_at = Some(Instant::now());
                }
                if let Some(d) = inner.devs.get_mut(&key) {
                    d.queue.push_back(QItem { job: None, payload: json!({"id": new_id(), "type": "stop", "job_id": job}), at: Instant::now() });
                }
                self.pollers.notify_all();
                self.relays.notify_all();
                Replied::Sent
            }
            Some("approve") => {
                let Some(d) = inner.devs.get_mut(&key) else { return Replied::NotFound };
                if !d.approvals {
                    return Replied::ApprovalsLocal;
                }
                let aid = match v.get("id") {
                    Some(Value::String(s)) if !s.is_empty() && s.len() <= 100 => json!(s),
                    Some(Value::Number(n)) if n.is_u64() => json!(n),
                    _ => return Replied::Bad("The approval has no valid id."),
                };
                let Some(yes) = v.get("yes").and_then(Value::as_bool) else { return Replied::Bad("The approval needs yes or no.") };
                d.queue.push_back(QItem { job: None, payload: json!({"id": new_id(), "type": "approval_reply", "job_id": job, "approval": aid, "approval_id": aid, "yes": yes}), at: Instant::now() });
                self.pollers.notify_all();
                Replied::Sent
            }
            Some("answer") => {
                let Some(d) = inner.devs.get_mut(&key) else { return Replied::NotFound };
                let aid = match v.get("id") {
                    Some(Value::String(s)) if !s.is_empty() && s.len() <= 100 => json!(s),
                    Some(Value::Number(n)) if n.is_u64() => json!(n),
                    _ => return Replied::Bad("The question has no valid id."),
                };
                let payload = if v.get("cancelled").and_then(Value::as_bool) == Some(true) {
                    json!({"id": new_id(), "type": "answer_reply", "job_id": job, "question_id": aid, "cancelled": true})
                } else if let Some(answers) = v.get("answers").and_then(Value::as_array) {
                    json!({"id": new_id(), "type": "answer_reply", "job_id": job, "question_id": aid, "answers": answers})
                } else {
                    return Replied::Bad("The question needs answers or cancelled: true.");
                };
                d.queue.push_back(QItem { job: None, payload, at: Instant::now() });
                self.pollers.notify_all();
                Replied::Sent
            }
            _ => Replied::Bad("Only stop, approve and answer can be sent to a running request."),
        }
    }

    /// A whitelisted settings question for a desktop. Returns the job.
    pub fn submit_op(&self, tenant: &str, want: Option<&str>, inner_op: Value) -> Result<String, SendErr> {
        let mut g = self.lock();
        let now = Instant::now();
        let dev = self.pick(&g, tenant, want, now).ok_or(SendErr::Offline)?;
        let inner = &mut *g;
        if inner.jobs.len() >= MAX_JOBS_ALL {
            return Err(SendErr::Limit("The server is relaying as many requests as it can right now. Try again in a moment."));
        }
        if inner.jobs.values().filter(|j| j.kind == Kind::Op && j.device == dev && j.tenant == tenant).count() >= MAX_OPS_PER_DEVICE {
            return Err(SendErr::Limit("Your computer is answering other questions. Try again in a moment."));
        }
        let id = new_id();
        let d = inner.devs.get_mut(&(tenant.to_string(), dev.clone())).expect("picked");
        if d.queue.len() >= MAX_QUEUED_PER_DEVICE {
            return Err(SendErr::Limit("Too many requests are waiting for this computer."));
        }
        d.queue.push_back(QItem { job: Some(id.clone()), payload: json!({"id": id, "type": "op", "inner": inner_op}), at: now });
        inner.jobs.insert(id.clone(), Job { tenant: tenant.into(), device: dev, kind: Kind::Op, created: now, delivered: false, last_activity: now, events: VecDeque::new(), pending: 0, total: 0, done: false, cancel: false, cancel_at: None, fail: None, result: None });
        self.pollers.notify_all();
        Ok(id)
    }

    /// The desktop's answer to a settings question (the job is removed afterwards, answered or not).
    pub fn wait_result(&self, job: &str, control: bool) -> Result<Value, (&'static str, String)> {
        let mut g = self.lock();
        // a control question waits for a person's answer on the computer (90 s there), so it waits a little longer than that
        let end = Instant::now() + if control { self.cfg.op_wait * 2 + Duration::from_secs(10) } else { self.cfg.op_wait };
        let r = loop {
            let now = Instant::now();
            let inner = &mut *g;
            let seen = inner.jobs.get(job).and_then(|j| inner.devs.get(&(j.tenant.clone(), j.device.clone()))).map(|d| if d.in_poll { now } else { d.last_seen });
            let Some(j) = inner.jobs.get_mut(job) else { break Err(("closed", "This request is no longer running.".to_string())) };
            if j.done {
                break match (&j.fail, j.result.take()) {
                    (Some((c, m)), _) => Err((*c, m.clone())),
                    (None, Some(r)) => Ok(r),
                    (None, None) => Err(("desktop_error", "Your computer gave no answer.".into())),
                };
            }
            if !j.delivered && now.saturating_duration_since(j.created) >= self.cfg.job_expiry {
                break Err(("desktop_timeout", "Your computer did not pick up the request in time.".into()));
            }
            if j.delivered && seen.is_none_or(|s| now.saturating_duration_since(s) >= self.cfg.lost_after) {
                break Err(("desktop_lost", "The connection to your computer was lost.".into()));
            }
            if now >= end {
                break Err(("desktop_timeout", "Your computer did not answer in time.".into()));
            }
            g = self.relays.wait_timeout(g, (end - now).min(self.cfg.tick)).unwrap_or_else(|e| e.into_inner()).0;
        };
        // never run late: what was not picked up is taken back
        let inner = &mut *g;
        if let Some(j) = inner.jobs.remove(job) {
            if let Some(d) = inner.devs.get_mut(&(j.tenant, j.device)) {
                d.queue.retain(|q| q.job.as_deref() != Some(job));
            }
        }
        r
    }

    /// The person's linked computers, for the web page: the desktop credentials the person has (`creds`: id, name, last used) with
    /// what the link knows about each.
    pub fn status(&self, tenant: &str, creds: &[(String, String, u64)]) -> Vec<Value> {
        let g = self.lock();
        let now = Instant::now();
        let mut out: Vec<Value> = creds
            .iter()
            .map(|(id, cred_name, used)| match g.devs.get(&(tenant.to_string(), id.clone())) {
                Some(d) => json!({"id": id, "name": if d.name.is_empty() { cred_name } else { &d.name }, "online": self.online(d, now), "last_seen": d.last_seen_unix, "caps": d.caps}),
                None => json!({"id": id, "name": cred_name, "online": false, "last_seen": used, "caps": {}}),
            })
            .collect();
        out.sort_by(|a, b| b["online"].as_bool().cmp(&a["online"].as_bool()).then(b["last_seen"].as_u64().cmp(&a["last_seen"].as_u64())));
        out
    }

    /// A person's computers and jobs are gone (the account was deleted).
    pub fn purge_tenant(&self, tenant: &str) {
        let mut g = self.lock();
        g.devs.retain(|(t, _), _| t != tenant);
        self.close_jobs(&mut g, |j| j.tenant == tenant, "account_deleted", "This account was deleted.");
    }

    /// One computer is signed out (its credential was revoked): it is forgotten and its jobs end.
    pub fn purge_device(&self, tenant: &str, dev: &str) {
        let mut g = self.lock();
        g.devs.remove(&(tenant.to_string(), dev.to_string()));
        self.close_jobs(&mut g, |j| j.tenant == tenant && j.device == dev, "desktop_revoked", "This computer was signed out of your account.");
    }

    fn close_jobs(&self, g: &mut Inner, which: impl Fn(&Job) -> bool, code: &'static str, msg: &str) {
        for j in g.jobs.values_mut().filter(|j| which(j) && !j.done) {
            j.fail = Some((code, msg.to_string()));
            j.cancel = true;
            j.done = true;
        }
        self.relays.notify_all();
        self.pollers.notify_all();
    }

    /// Housekeeping: computers not heard of for an hour are forgotten, old memories of ended jobs go.
    pub fn sweep(&self) {
        let mut g = self.lock();
        let now = Instant::now();
        g.devs.retain(|_, d| d.in_poll || now.saturating_duration_since(d.last_seen) < Duration::from_secs(3600));
        g.ended.retain(|_, (_, _, at)| now.saturating_duration_since(*at) < Duration::from_secs(600));
        let live: std::collections::HashSet<String> = g.jobs.keys().cloned().collect();
        g.streams.retain(|_, job| live.contains(job));
    }

    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize, usize) {
        let g = self.lock();
        (g.devs.len(), g.jobs.len(), g.streams.len())
    }

    /// Tests: whether a device's poll is open, and how many items wait for it.
    #[cfg(test)]
    pub fn queued(&self, tenant: &str, dev: &str) -> usize {
        self.lock().devs.get(&(tenant.to_string(), dev.to_string())).map(|d| d.queue.len()).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// HTTP

fn err_body(code: &str, msg: &str) -> Value {
    json!({"ok": false, "error": code, "message": msg})
}

fn send_err(e: SendErr) -> (&'static str, String) {
    match e {
        SendErr::Offline => ("desktop_offline", OFFLINE_MESSAGE.to_string()),
        SendErr::Busy => ("desktop_busy", "Your computer is already running a web request. Wait for it to finish or press Stop.".to_string()),
        SendErr::Limit(m) => ("limit", m.to_string()),
    }
}

/// POST /api/link/poll (a desktop's credential).
pub fn poll_http(app: &App, who: &Who, id: &str, v: &Value) -> Result<Value, Fail> {
    let jobs = app.link.poll(&who.tenant, id, v.get("name").and_then(Value::as_str).unwrap_or(""), v.get("caps").unwrap_or(&Value::Null))?;
    Ok(json!({"ok": true, "jobs": jobs}))
}

/// POST /api/link/events (a desktop's credential): `{job_id, events: [...], done?}` -> `{ok, cancel}`.
pub fn events_http(app: &App, who: &Who, id: &str, v: &Value) -> Result<Value, Fail> {
    let job = v.get("job_id").and_then(Value::as_str).filter(|j| !j.is_empty() && j.len() <= 64).ok_or_else(|| Fail::new(400, "The post has no job_id."))?;
    let events = match v.get("events") {
        Some(Value::Array(a)) if a.len() <= EVENTS_PER_POST && a.iter().all(Value::is_object) => a.as_slice(),
        None => &[],
        _ => return Err(Fail::new(400, format!("events must be a list of at most {EVENTS_PER_POST} objects."))),
    };
    let done = v.get("done").and_then(Value::as_bool).unwrap_or(false);
    match app.link.events(&who.tenant, id, job, events, done) {
        Ok(cancel) => Ok(json!({"ok": true, "cancel": cancel})),
        Err(f) if f.status == 404 => Err(Fail { status: 404, error: "unknown_job".into(), retry: None }),
        Err(f) => Err(f),
    }
}

/// POST /api/link/status (a person signed in with Firebase).
pub fn status_http(app: &App, who: &Who) -> Result<Value, Fail> {
    let creds: Vec<(String, String, u64)> = match &app.devices {
        None => Vec::new(),
        Some(d) => d
            .list(&who.tenant)
            .map_err(|e| {
                eprintln!("zo-server: {e}");
                Fail::new(503, "The storage is not reachable right now. Try again in a moment.")
            })?
            .into_iter()
            .filter(|d| d.kind.unwrap_or(if d.scope == crate::devices::Scope::Sync { crate::devices::Kind::Desktop } else { crate::devices::Kind::Cli }) == crate::devices::Kind::Desktop)
            .map(|d| (d.id, d.name, d.last_used))
            .collect(),
    };
    Ok(json!({"ok": true, "devices": app.link.status(&who.tenant, &creds)}))
}

fn text_field(v: &Value, k: &str, max: usize) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(|s| s.chars().take(max).collect())
}

/// The settings question as the desktop gets it: only known names and only known fields, with sizes and values checked.
pub fn clean_inner(inner: &Value) -> Result<Value, &'static str> {
    let op = inner.get("op").and_then(Value::as_str).unwrap_or("");
    if !OPS.contains(&op) {
        return Err("That question cannot be sent to your computer from the web.");
    }
    let mut out = Map::new();
    out.insert("op".into(), json!(op));
    let mut put = |k: &str, max: usize| {
        if let Some(s) = text_field(inner, k, max) {
            out.insert(k.into(), json!(strip_name(&s)));
        }
    };
    match op {
        "profile_use" => put("id", 100),
        "models" => put("provider", 100),
        "select" => {
            put("provider", 100);
            put("model", 200);
            put("effort", 20);
        }
        "fs_list" | "fs_read" => match inner.get("path") {
            None | Some(Value::Null) => {}
            Some(Value::String(p)) if p.chars().count() <= MAX_PATH_CHARS && !p.chars().any(char::is_control) => {
                out.insert("path".into(), json!(p));
            }
            _ => return Err("The path is not valid."),
        },
        "mode" => {
            if let Some(m) = inner.get("set") {
                // the web page may switch between Auto and Manual; Bypass is never set from the web
                match m.as_str().map(|s| s.trim().to_ascii_lowercase()).as_deref() {
                    Some(s @ ("auto" | "manual")) => {
                        out.insert("set".into(), json!(s));
                    }
                    _ => return Err("Only Auto and Manual can be chosen from the web."),
                }
            }
        }
        _ => {}
    }
    Ok(Value::Object(out))
}

/// POST /api/op with `{"op": "desktop_op", "device"?, "inner": {...}}`: returns (status, body).
pub fn op_http(app: &App, who: &Who, v: &Value) -> (u16, Value) {
    let inner = match clean_inner(v.get("inner").unwrap_or(&Value::Null)) {
        Ok(i) => i,
        Err(m) => return (400, err_body("not_allowed", m)),
    };
    let want = v.get("device").and_then(Value::as_str).filter(|d| !d.is_empty() && d.len() <= 64);
    let inner_op = inner.get("op").and_then(Value::as_str).unwrap_or("").to_string();
    let inner_op = inner_op.as_str();
    let job = match app.link.submit_op(&who.tenant, want, inner) {
        Ok(j) => j,
        Err(e) => {
            let (code, msg) = send_err(e);
            return (if code == "limit" { 429 } else { 503 }, err_body(code, &msg));
        }
    };
    let control = is_control(inner_op);
    match app.link.wait_result(&job, control) {
        Ok(r) => (200, json!({"ok": true, "result": r})),
        Err((code, msg)) => (503, err_body(code, &msg)),
    }
}

/// The lines of an answer that never reached a desktop, as an ordinary (not streamed) NDJSON body.
fn ndjson(app: &App, req: Request, lines: &[Value]) {
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let r = Response::from_string(body).with_header(crate::header("Content-Type", "application/x-ndjson")).with_header(crate::header("Cache-Control", "no-store"));
    crate::send_response(app, req, r);
}

/// POST /api/send with `"target": "desktop"`: the message goes to the person's computer and what it does is streamed back as
/// NDJSON, like an answer made on the server.
pub fn send_http(app: &Arc<App>, req: Request, v: Value, who: &Who) {
    let text = v.get("text").and_then(Value::as_str).unwrap_or("");
    if text.trim().is_empty() {
        return crate::fail(app, req, Fail::new(400, "The message is empty."));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return crate::fail(app, req, Fail::new(413, format!("The message is too long (at most {MAX_TEXT_CHARS} characters).")));
    }
    let mut payload = json!({"text": text});
    if let Some(c) = v.get("chat") {
        match c.as_str() {
            Some(c) if c.len() <= 100 && !c.chars().any(char::is_control) => payload["chat"] = json!(c),
            None if c.is_null() => {}
            _ => return crate::fail(app, req, Fail::new(400, "The chat id is not valid.")),
        }
    }
    if let Some(a) = v.get("attachments") {
        match a {
            Value::Array(l) if l.len() <= 20 && l.iter().all(Value::is_object) => payload["attachments"] = a.clone(),
            Value::Null => {}
            _ => return crate::fail(app, req, Fail::new(400, "The attachments are not valid.")),
        }
    }
    if let Some(m) = v.get("mode").and_then(Value::as_str).map(|m| m.trim().to_ascii_lowercase()).filter(|m| ["auto", "manual", "bypass"].contains(&m.as_str())) {
        payload["mode"] = json!(m); // the desktop caps it (Bypass only when its owner allowed that on the computer)
    }
    let want = v.get("device").and_then(Value::as_str).filter(|d| !d.is_empty() && d.len() <= 64);
    let stream = app.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let (job, name) = match app.link.submit_send(&who.tenant, want, payload, stream) {
        Ok(x) => x,
        Err(e) => {
            let (code, msg) = send_err(e);
            return ndjson(app, req, &[json!({"event": "error", "error": code, "message": msg}), json!({"event": "done"})]);
        }
    };
    let mut out = req.into_writer();
    let cors = if app.no_auth { "" } else { "Access-Control-Allow-Origin: *\r\n" };
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Accel-Buffering: no\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n{cors}\r\n");
    let chunk = |out: &mut Box<dyn Write + Send>, line: &str| -> bool { out.write_all(format!("{:x}\r\n{line}\r\n", line.len()).as_bytes()).is_ok() && out.flush().is_ok() };
    let mut ok = out.write_all(head.as_bytes()).is_ok() && chunk(&mut out, &format!("{}\n", json!({"event": "stream", "id": stream, "target": "desktop", "device": name})));
    while ok {
        match app.link.relay_wait(&job, app.link.cfg.ping_every) {
            Relay::Idle => ok = chunk(&mut out, &format!("{}\n", json!({"event": "ping"}))),
            Relay::Events(evs) => {
                for e in evs {
                    ok = ok && chunk(&mut out, &format!("{e}\n"));
                }
            }
            Relay::End(evs, why) => {
                for e in evs {
                    ok = ok && chunk(&mut out, &format!("{e}\n"));
                }
                match why {
                    Some(("stopped", _)) => ok = ok && chunk(&mut out, &format!("{}\n", json!({"event": "stopped"}))),
                    Some((code, msg)) => ok = ok && chunk(&mut out, &format!("{}\n", json!({"event": "error", "error": code, "message": msg}))),
                    None => {}
                }
                let _ = ok && chunk(&mut out, &format!("{}\n", json!({"event": "done"})));
                break;
            }
        }
    }
    let _ = out.write_all(b"0\r\n\r\n");
    let _ = out.flush();
    // a page that went away while the job still runs stops it
    app.link.cancel_job(&job);
    app.link.finish(&job, stream);
}

/// A line for a running job from `/api/reply`: (status, body).
pub fn reply_http(app: &App, who: &Who, stream: u64, v: &Value) -> (u16, Value) {
    match app.link.reply(&who.tenant, stream, v) {
        Replied::Sent => (200, json!({"ok": true})),
        Replied::NotFound => (404, json!({"ok": false})),
        Replied::ApprovalsLocal => (409, err_body("approvals_on_computer", "This approval is answered on your computer. Turn on web approvals in ZO-AI's settings there to answer it here.")),
        Replied::Bad(m) => (400, err_body("bad_reply", m)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast() -> Link {
        Link::new(Config { poll_wait: Duration::from_millis(60), online_window: Duration::from_millis(300), job_expiry: Duration::from_millis(250), lost_after: Duration::from_millis(400), op_wait: Duration::from_millis(500), stop_grace: Duration::from_millis(100), ping_every: Duration::from_millis(100), tick: Duration::from_millis(10), ..Config::default() })
    }

    fn up(l: &Link, t: &str, d: &str) {
        l.poll(t, d, d, &json!({})).unwrap();
    }

    #[test]
    fn a_second_poll_of_the_same_device_ends_the_first_at_once() {
        let l = Arc::new(Link::new(Config { poll_wait: Duration::from_secs(5), ..Config::default() }));
        let l1 = l.clone();
        let t0 = Instant::now();
        let first = std::thread::spawn(move || l1.poll("t", "d", "n", &json!({})).unwrap());
        std::thread::sleep(Duration::from_millis(100));
        let l2 = l.clone();
        let second = std::thread::spawn(move || l2.poll("t", "d", "n", &json!({})).unwrap());
        assert!(first.join().unwrap().is_empty());
        assert!(t0.elapsed() < Duration::from_secs(3), "the first poll was released, not left to run its 5 s");
        // the newer poll is the open one: a job is delivered to it
        std::thread::sleep(Duration::from_millis(50));
        l.submit_send("t", None, json!({"text": "x"}), 1).unwrap();
        let got = second.join().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["type"], "send");
    }

    #[test]
    fn presence_follows_the_polls_and_belongs_to_one_tenant() {
        let l = fast();
        assert_eq!(l.submit_send("t1", None, json!({"text": "x"}), 1), Err(SendErr::Offline));
        up(&l, "t1", "d1");
        assert!(l.status("t1", &[("d1".into(), "c".into(), 0)])[0]["online"].as_bool().unwrap());
        assert!(!l.status("t2", &[("d1".into(), "c".into(), 0)])[0]["online"].as_bool().unwrap(), "another tenant's id is not this device");
        assert_eq!(l.submit_send("t2", None, json!({"text": "x"}), 2), Err(SendErr::Offline));
        assert_eq!(l.submit_send("t1", Some("d9"), json!({"text": "x"}), 3), Err(SendErr::Offline));
        std::thread::sleep(Duration::from_millis(350));
        assert!(!l.status("t1", &[("d1".into(), "c".into(), 0)])[0]["online"].as_bool().unwrap());
        assert_eq!(l.submit_send("t1", None, json!({"text": "x"}), 4), Err(SendErr::Offline));
    }

    #[test]
    fn the_most_recently_seen_online_computer_is_the_default_and_a_chosen_one_is_used() {
        let l = fast();
        up(&l, "t", "old");
        std::thread::sleep(Duration::from_millis(30));
        up(&l, "t", "new");
        let (job, name) = l.submit_send("t", None, json!({"text": "x"}), 1).unwrap();
        assert_eq!(name, "new");
        l.finish(&job, 1);
        let (_, name) = l.submit_send("t", Some("old"), json!({"text": "x"}), 2).unwrap();
        assert_eq!(name, "old");
    }

    #[test]
    fn at_most_a_few_jobs_per_person_and_one_per_computer() {
        let l = Arc::new(Link::new(Config { online_window: Duration::from_secs(30), poll_wait: Duration::from_millis(10), ..Config::default() }));
        for d in ["a", "b", "c", "d"] {
            up(&l, "t", d);
        }
        let mut ok = 0;
        let mut limit = false;
        for (n, d) in ["a", "b", "c", "d"].iter().enumerate() {
            match l.submit_send("t", Some(d), json!({"text": "x"}), n as u64) {
                Ok(_) => ok += 1,
                Err(SendErr::Limit(_)) => limit = true,
                Err(e) => panic!("{e:?}"),
            }
        }
        assert_eq!((ok, limit), (MAX_SENDS_PER_TENANT, true));
        assert_eq!(l.submit_send("t", Some("a"), json!({"text": "x"}), 99), Err(SendErr::Busy));
        // another person is not affected
        up(&l, "t2", "z");
        assert!(l.submit_send("t2", None, json!({"text": "x"}), 100).is_ok());
    }

    #[test]
    fn polls_and_posts_are_rate_limited_per_device_and_polls_per_server_are_capped() {
        let l = fast();
        let mut limited = None;
        for n in 0..(POLLS_PER_MIN + 5) {
            if let Err(f) = l.poll("t", "d", "n", &json!({})) {
                limited = Some((n, f.status, f.retry));
                break;
            }
        }
        let (n, status, retry) = limited.expect("polling without end is refused");
        assert_eq!((n, status), (POLLS_PER_MIN, 429));
        assert!(retry.unwrap() >= 1);
        // another device is not affected
        assert!(l.poll("t", "other", "n", &json!({})).is_ok());
        // posts
        let (job, _) = l.submit_send("t", Some("other"), json!({"text": "x"}), 1).unwrap();
        l.poll("t", "other", "n", &json!({})).unwrap();
        let mut refused = false;
        for _ in 0..(EVENT_POSTS_PER_MIN + 5) {
            if let Err(f) = l.events("t", "other", &job, &[json!({"event": "delta"})], false) {
                assert_eq!(f.status, 429);
                refused = true;
                break;
            }
        }
        assert!(refused);
        // the number of computers per person
        for n in 0..MAX_DEVICES_PER_TENANT {
            let _ = l.poll("full", &format!("d{n}"), "n", &json!({}));
        }
        assert_eq!(l.poll("full", "one-too-many", "n", &json!({})).unwrap_err().status, 409);
    }

    #[test]
    fn a_job_may_not_send_more_than_the_limits_and_is_then_stopped() {
        let l = fast();
        up(&l, "t", "d");
        let (job, _) = l.submit_send("t", None, json!({"text": "x"}), 1).unwrap();
        l.poll("t", "d", "n", &json!({})).unwrap();
        let big = json!({"event": "delta", "text": "x".repeat(1_000_000)});
        let mut stopped = false;
        for _ in 0..12 {
            // nobody reads: the unread output is capped
            if l.events("t", "d", &job, &[big.clone()], false).unwrap() {
                stopped = true;
                break;
            }
        }
        assert!(stopped, "more than {PENDING_BYTES} unread bytes stop the job");
        match l.relay_wait(&job, Duration::from_millis(50)) {
            Relay::End(_, Some((code, _))) => assert_eq!(code, "too_much_output"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn expiry_is_by_the_clock_a_job_picked_up_in_time_runs_and_a_lost_computer_is_given_up() {
        let l = fast();
        up(&l, "t", "d");
        // picked up in time: delivered, then never expires by the pickup rule
        let (job, _) = l.submit_send("t", None, json!({"text": "x"}), 1).unwrap();
        let got = l.poll("t", "d", "n", &json!({})).unwrap();
        assert_eq!(got[0]["id"], job.as_str());
        std::thread::sleep(Duration::from_millis(300)); // longer than the pickup limit
        l.poll("t", "d", "n", &json!({})).unwrap();
        assert!(matches!(l.relay_wait(&job, Duration::from_millis(20)), Relay::Idle), "a running job is not expired");
        // the computer goes silent: after the lost limit the job is given up
        std::thread::sleep(Duration::from_millis(500));
        match l.relay_wait(&job, Duration::from_millis(50)) {
            Relay::End(_, Some((code, _))) => assert_eq!(code, "desktop_lost"),
            other => panic!("{other:?}"),
        }
        // an undelivered job expires and is never handed over afterwards
        let (job2, _) = l.submit_send("t", None, json!({"text": "y"}), 2).unwrap_or_else(|_| {
            up(&l, "t", "d");
            l.submit_send("t", None, json!({"text": "y"}), 2).unwrap()
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(l.poll("t", "d", "n", &json!({})).unwrap().is_empty());
        match l.relay_wait(&job2, Duration::from_millis(50)) {
            Relay::End(_, Some((code, _))) => assert_eq!(code, "desktop_timeout"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn purging_a_person_or_a_computer_ends_their_jobs() {
        let l = fast();
        up(&l, "t", "d");
        let (job, _) = l.submit_send("t", None, json!({"text": "x"}), 1).unwrap();
        l.purge_device("t", "d");
        match l.relay_wait(&job, Duration::from_millis(50)) {
            Relay::End(_, Some((code, _))) => assert_eq!(code, "desktop_revoked"),
            other => panic!("{other:?}"),
        }
        l.finish(&job, 1);
        assert_eq!(l.submit_send("t", None, json!({"text": "x"}), 2), Err(SendErr::Offline));
        up(&l, "t", "d");
        let (job, _) = l.submit_send("t", None, json!({"text": "x"}), 3).unwrap();
        l.purge_tenant("t");
        match l.relay_wait(&job, Duration::from_millis(50)) {
            Relay::End(_, Some((code, _))) => assert_eq!(code, "account_deleted"),
            other => panic!("{other:?}"),
        }
        l.finish(&job, 3);
        assert_eq!(l.counts(), (0, 0, 0));
    }

    #[test]
    fn the_names_and_capabilities_a_computer_reports_are_cleaned() {
        let (caps, approvals) = clean_caps(&json!({"claude_code": "yes", "approvals_from_web": true, "version": "1.0\u{202e}evil", "os": "x".repeat(500), "other": 1}));
        assert!(approvals);
        assert_eq!(caps["claude_code"], false, "only a real boolean counts");
        assert_eq!(caps["version"], "1.0evil");
        assert_eq!(caps["os"].as_str().unwrap().len(), 40);
        assert!(caps.get("other").is_none());
        assert_eq!(clean_caps(&Value::Null).0["approvals_from_web"], false);
    }
}
