//! Data model and persistence for Zohara Chat projects.
//!
//! A project owns its instructions, attached context, memory and schedules.
//! Memory is loaded into the project's working tree on every session, so the
//! agent already knows it before the first message.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Schedule {
    pub text: String,
    #[serde(default)]
    pub when: String,
    #[serde(default = "default_repeat")]
    pub repeat: String,
}

fn default_repeat() -> String {
    "once".to_string()
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            text: String::new(),
            when: String::new(),
            repeat: default_repeat(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub text: String,
    #[serde(default)]
    pub when: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub context: Vec<Attachment>,
    #[serde(default)]
    pub schedules: Vec<Schedule>,
    #[serde(default)]
    pub memory: Vec<MemoryEntry>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
    #[serde(default)]
    pub pinned: bool,
}

impl Project {
    pub fn new(name: impl Into<String>, goal: impl Into<String>) -> Self {
        let now = now_millis();
        Self {
            id: format!("p{}", now),
            name: name.into(),
            goal: goal.into(),
            instructions: String::new(),
            context: Vec::new(),
            schedules: Vec::new(),
            memory: Vec::new(),
            created: now,
            updated: now,
            pinned: false,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub projects: Vec<Project>,
}

impl Store {
    pub fn path() -> PathBuf {
        crate::audit::data_dir().join("projects.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                // never silently start empty over a file we could not read: keep it aside so nothing is lost
                let aside = path.with_extension("json.broken");
                eprintln!("zohara-chat: {} is damaged ({e}); kept as {}", path.display(), aside.display());
                let _ = fs::rename(&path, &aside);
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            // write to a side file and rename, so a crash mid-write cannot leave a half file
            let tmp = path.with_extension("json.tmp");
            if let Err(e) = fs::write(&tmp, raw).and_then(|_| fs::rename(&tmp, &path)) {
                eprintln!("zohara-chat: could not save {}: {e}", path.display());
            }
        }
    }

    pub fn add(&mut self, name: String, goal: String) -> String {
        let project = Project::new(name, goal);
        let id = project.id.clone();
        self.projects.insert(0, project);
        id
    }

    pub fn get(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Project> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// Pinned first, then newest-updated first.
    #[allow(dead_code)]
    pub fn ordered(&self) -> Vec<Project> {
        let mut list = self.projects.clone();
        list.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)));
        list
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Human date for a millisecond timestamp: "2 days ago", or "Nov 2" past a month.
pub fn relative(ts: u64) -> String {
    let now = now_millis();
    let secs = now.saturating_sub(ts) / 1000;

    if secs < 60 {
        return "just now".to_string();
    }
    if secs < 3600 {
        let m = secs / 60;
        return format!("{} min ago", m);
    }
    if secs < 86_400 {
        let h = secs / 3600;
        return if h == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", h)
        };
    }

    let days = secs / 86_400;
    if days < 30 {
        return if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", days)
        };
    }

    let (_, month, day) = civil_from_days((ts / 1000 / 86_400) as i64);
    format!("{} {}", MONTHS[(month - 1) as usize], day)
}
