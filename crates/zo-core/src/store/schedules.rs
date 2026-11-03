//! Scheduled tasks: things ZO-AI should do later, once or on a repeat. Made on the Scheduled page or by asking in a chat.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    /// A short name for lists.
    pub title: String,
    /// What ZO-AI is asked to do when it is time, written as an instruction.
    pub prompt: String,
    /// When it runs next, in milliseconds since 1970.
    pub next_run: u64,
    /// once, hourly, daily or weekly.
    pub repeat: String,
    pub enabled: bool,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub last_run: Option<u64>,
    /// The chat the last run left behind.
    #[serde(default)]
    pub last_chat: Option<String>,
}

pub const REPEATS: [&str; 4] = ["once", "hourly", "daily", "weekly"];

fn path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into())).join(".local/share")
    });
    base.join("zo-ai").join("schedules.json")
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Soonest first.
pub fn load() -> Vec<Task> {
    let mut v: Vec<Task> = std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.sort_by_key(|t| t.next_run);
    v
}

fn write(all: &[Task]) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("json.tmp");
    if let Ok(s) = serde_json::to_string_pretty(all) {
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// Turns "tomorrow 9am", "in 2 hours", "2026-11-05 18:30" or "next monday" into a time, using the system's `date`.
pub fn parse_when(text: &str) -> Result<u64, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("Say when, for example \"tomorrow 9am\" or \"in 2 hours\".".into());
    }
    if t.len() > 80 || t.starts_with('-') {
        return Err("That doesn't look like a time.".into());
    }
    let out = Command::new("date").args(["-d", t, "+%s"]).env("LC_ALL", "C").output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("I couldn't read \"{t}\" as a time. Try \"tomorrow 9am\", \"in 2 hours\" or \"2026-11-05 18:30\"."));
    }
    let secs: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().map_err(|_| "That doesn't look like a time.".to_string())?;
    Ok(secs * 1000)
}

/// "Wed 04 Nov, 09:00".
pub fn format_when(ms: u64) -> String {
    Command::new("date").args(["-d", &format!("@{}", ms / 1000), "+%a %d %b, %H:%M"]).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "?".into())
}

pub fn add(title: &str, prompt: &str, next_run: u64, repeat: &str, project: Option<String>) -> Result<Task, String> {
    if prompt.trim().is_empty() {
        return Err("Say what ZO-AI should do.".into());
    }
    if !REPEATS.contains(&repeat) {
        return Err(format!("repeat must be one of: {}", REPEATS.join(", ")));
    }
    let now = now_millis();
    let title = if title.trim().is_empty() { prompt.trim().chars().take(48).collect() } else { title.trim().to_string() };
    let task = Task { id: format!("t{now}{}", load().len()), title, prompt: prompt.trim().to_string(), next_run, repeat: repeat.into(), enabled: true, project, created: now, last_run: None, last_chat: None };
    let mut all = load();
    all.push(task.clone());
    write(&all);
    Ok(task)
}

pub fn remove(id: &str) -> bool {
    let mut all = load();
    let before = all.len();
    all.retain(|t| t.id != id);
    write(&all);
    all.len() != before
}

/// Makes a task due right now (the "Run now" button).
pub fn run_now(id: &str) {
    let mut all = load();
    if let Some(t) = all.iter_mut().find(|t| t.id == id) {
        t.enabled = true;
        t.next_run = now_millis();
    }
    write(&all);
}

pub fn set_enabled(id: &str, on: bool) {
    let mut all = load();
    if let Some(t) = all.iter_mut().find(|t| t.id == id) {
        t.enabled = on;
        if on && t.repeat == "once" && t.next_run < now_millis() {
            t.next_run = now_millis() + 60_000; // switched on again after it was due: run it soon
        }
    }
    write(&all);
}

/// Enabled tasks whose time has come.
pub fn due(now: u64) -> Vec<Task> {
    load().into_iter().filter(|t| t.enabled && t.next_run <= now).collect()
}

fn step(repeat: &str) -> Option<u64> {
    match repeat {
        "hourly" => Some(3_600_000),
        "daily" => Some(86_400_000),
        "weekly" => Some(7 * 86_400_000),
        _ => None,
    }
}

/// Record that a task ran: a repeating one moves to its next time (never into the past), a one-off switches itself off.
pub fn mark_ran(id: &str, now: u64, chat: Option<String>) {
    let mut all = load();
    if let Some(t) = all.iter_mut().find(|t| t.id == id) {
        t.last_run = Some(now);
        t.last_chat = chat;
        match step(&t.repeat) {
            Some(s) => {
                while t.next_run <= now {
                    t.next_run += s;
                }
            }
            None => t.enabled = false,
        }
    }
    write(&all);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_dir(name: &str, f: impl FnOnce()) {
        let _g = super::super::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zo-{name}-{}", std::process::id()));
        std::env::set_var("XDG_DATA_HOME", &dir);
        f();
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn times_are_read_from_plain_words_and_refused_when_nonsense() {
        let now = now_millis();
        let in_2h = parse_when("now + 2 hours").unwrap();
        assert!(in_2h > now + 7_100_000 && in_2h < now + 7_300_000);
        assert_eq!(parse_when("@1700000000").unwrap(), 1_700_000_000_000);
        assert!(parse_when("whenever it feels right").is_err());
        assert!(parse_when("").is_err());
        assert!(parse_when("-f /etc/passwd").is_err());
    }

    #[test]
    fn a_task_is_added_listed_soonest_first_and_removed() {
        with_temp_dir("sched-a", || {
            add("later", "do b", 2_000, "daily", None).unwrap();
            add("", "do a thing that has a long description", 1_000, "once", Some("p1".into())).unwrap();
            let all = load();
            assert_eq!(all[0].prompt, "do a thing that has a long description");
            assert_eq!(all[0].title, "do a thing that has a long description");
            assert_eq!(all[0].project.as_deref(), Some("p1"));
            assert!(add("x", "  ", 1, "once", None).is_err());
            assert!(add("x", "y", 1, "monthly", None).is_err());
            assert!(remove(&all[0].id));
            assert_eq!(load().len(), 1);
        });
    }

    #[test]
    fn due_tasks_run_once_or_move_to_their_next_time() {
        with_temp_dir("sched-b", || {
            let once = add("once", "a", 1_000, "once", None).unwrap();
            let daily = add("daily", "b", 1_000, "daily", None).unwrap();
            let future = add("future", "c", 10_000_000_000_000, "once", None).unwrap();
            let now = 5 * 86_400_000 + 500;
            assert_eq!(due(now).len(), 2);
            mark_ran(&once.id, now, Some("c1".into()));
            mark_ran(&daily.id, now, None);
            let all = load();
            let o = all.iter().find(|t| t.id == once.id).unwrap();
            assert!(!o.enabled && o.last_run == Some(now) && o.last_chat.as_deref() == Some("c1"));
            let d = all.iter().find(|t| t.id == daily.id).unwrap();
            assert!(d.enabled && d.next_run > now && d.next_run <= now + 86_400_000, "next daily run is in the future, not a backlog");
            assert!(due(now).is_empty());
            assert!(all.iter().any(|t| t.id == future.id && t.enabled));
            set_enabled(&once.id, true);
            assert!(load().iter().find(|t| t.id == once.id).unwrap().enabled);
        });
    }
}
