//! The greeting on the home screen ("Good morning", "Still up?", "Back at it", ...).
//!
//! The wording lives in ONE file, `data/greetings.json`, which the three clients share: the desktop app uses this module, the web
//! server inlines the file into the page, and the Android app bundles a copy (a test keeps the copy equal to the source). The picking
//! rules below are written out the same way in `web/index.html` (`greetPick`) and in the Android `Greetings.kt`; the "golden" cases
//! in the JSON file are checked by all three, so a rule that drifts in one client fails a test there.
//!
//! Rules: a message has tags (time of day, weekday, gap). An empty tag list means "any". A gap tag other than "none" is for the
//! first open of a day only; when one matches, only those messages are candidates. Otherwise the candidates are the messages without a
//! gap tag whose time and weekday match. The pick is random among candidates, weighted a little towards the more specific ones,
//! and never the message of the last open.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;

/// The shared table, verbatim (the web server inlines this text; the Android copy must equal it).
pub const SOURCE: &str = include_str!("../data/greetings.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub time: Vec<String>,
    #[serde(default)]
    pub weekday: Vec<String>,
    #[serde(default)]
    pub gap: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Golden {
    pub hour: u32,
    pub weekday: String,
    pub gap: String,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Table {
    pub version: u32,
    pub max_len: usize,
    pub away_days: i64,
    pub back_days: i64,
    pub streak_days: u32,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub golden: Vec<Golden>,
}

/// The table, parsed once.
pub fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(SOURCE).expect("data/greetings.json is valid"))
}

pub const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// The time-of-day slot of an hour (0..=23).
pub fn slot(hour: u32) -> &'static str {
    match hour {
        5..=8 => "early",
        9..=11 => "morning",
        12..=13 => "noon",
        14..=17 => "afternoon",
        18..=20 => "evening",
        21..=23 => "night",
        _ => "late",
    }
}

/// How long the person has been away, seen at the first open of a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    None,
    First,
    Away,
    Back,
    Streak,
}

impl Gap {
    pub fn as_str(self) -> &'static str {
        match self {
            Gap::None => "none",
            Gap::First => "first",
            Gap::Away => "away",
            Gap::Back => "back",
            Gap::Streak => "streak",
        }
    }
    pub fn parse(s: &str) -> Gap {
        match s {
            "first" => Gap::First,
            "away" => Gap::Away,
            "back" => Gap::Back,
            "streak" => Gap::Streak,
            _ => Gap::None,
        }
    }
}

/// What is remembered between opens (a small JSON file).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// The local day number (days since 1970-01-01) of the last open.
    #[serde(default)]
    pub last_day: Option<i64>,
    /// Days in a row with at least one open, counting the last open.
    #[serde(default)]
    pub streak: u32,
    /// The message shown at the last open.
    #[serde(default)]
    pub last_id: Option<String>,
}

impl State {
    pub fn load(path: &Path) -> State {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string(self) {
            let _ = std::fs::write(path, t);
        }
    }
}

/// The local moment of an open: hour 0..=23, weekday 0=Monday..6=Sunday, day = days since 1970-01-01 in local time.
#[derive(Debug, Clone, Copy)]
pub struct Moment {
    pub hour: u32,
    pub weekday: u32,
    pub day: i64,
}

/// Days since 1970-01-01 of a calendar date (month 1..=12), for building a `Moment` from a clock that has no day number.
pub fn day_number(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The streak after an open on `today`, and the gap to report for it.
pub fn visit(t: &Table, st: &State, today: i64) -> (Gap, u32) {
    let Some(last) = st.last_day else { return (Gap::First, 1) };
    let days = today - last;
    if days <= 0 {
        return (Gap::None, st.streak.max(1)); // a second open on the same day (or a clock set back): nothing to announce
    }
    let streak = if days == 1 { st.streak + 1 } else { 1 };
    let gap = if days >= t.away_days {
        Gap::Away
    } else if days >= t.back_days {
        Gap::Back
    } else if streak >= t.streak_days {
        Gap::Streak
    } else {
        Gap::None
    };
    (gap, streak)
}

fn weekday_matches(tags: &[String], wd: u32) -> bool {
    tags.is_empty()
        || tags.iter().any(|t| match t.as_str() {
            "monday" => wd == 0,
            "friday" => wd == 4,
            "weekend" => wd >= 5,
            _ => false,
        })
}

/// The messages that fit this hour, weekday and gap, in table order.
pub fn candidates(t: &Table, hour: u32, weekday: u32, gap: Gap) -> Vec<&Message> {
    let s = slot(hour);
    let fits = |m: &&Message| (m.time.is_empty() || m.time.iter().any(|x| x == s)) && weekday_matches(&m.weekday, weekday);
    if gap != Gap::None {
        let special: Vec<&Message> = t.messages.iter().filter(|m| m.gap.iter().any(|g| g == gap.as_str())).filter(fits).collect();
        if !special.is_empty() {
            return special;
        }
    }
    t.messages.iter().filter(|m| m.gap.is_empty()).filter(fits).collect()
}

fn weight(m: &Message) -> f64 {
    1.0 + if m.weekday.is_empty() { 0.0 } else { 2.0 } + if m.time.is_empty() { 0.0 } else { 1.0 }
}

/// One of the candidates: never `last_id` (unless it is the only one), weighted towards the specific ones. `rnd` is in [0, 1).
pub fn pick<'a>(cands: &[&'a Message], last_id: Option<&str>, rnd: f64) -> Option<&'a Message> {
    let mut pool: Vec<&Message> = cands.to_vec();
    if pool.len() > 1 {
        pool.retain(|m| Some(m.id.as_str()) != last_id);
    }
    let total: f64 = pool.iter().map(|m| weight(m)).sum();
    let mut x = rnd.clamp(0.0, 0.999_999) * total;
    for m in &pool {
        x -= weight(m);
        if x < 0.0 {
            return Some(m);
        }
    }
    pool.last().copied()
}

/// The text with the name put in; without a name ", {name}" (and a bare "{name}") disappears.
pub fn render(m: &Message, name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        m.text.replace(", {name}", "").replace("{name}", "").trim().to_string()
    } else {
        m.text.replace("{name}", name)
    }
}

/// The first word of a display name, else the part of the e-mail / user name before any "@", capitalised ("" when neither is known).
pub fn first_name(display: &str, fallback: &str) -> String {
    let first = display.split_whitespace().next().unwrap_or("");
    let raw = if first.is_empty() { fallback.split('@').next().unwrap_or("").trim() } else { first };
    let mut c = raw.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The greeting for an open, and the state to remember. `rnd` is in [0, 1) (the clients pass a random number; tests pass fixed ones).
pub fn greet(t: &Table, st: &State, m: Moment, name: &str, rnd: f64) -> (String, State) {
    let (gap, streak) = visit(t, st, m.day);
    let cands = candidates(t, m.hour, m.weekday, gap);
    let chosen = pick(&cands, st.last_id.as_deref(), rnd);
    let text = chosen.map(|c| render(c, name)).unwrap_or_default();
    let next = State { last_day: Some(st.last_day.map_or(m.day, |d| d.max(m.day))), streak, last_id: chosen.map(|c| c.id.clone()) };
    (text, next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const GAPS: [Gap; 4] = [Gap::First, Gap::Away, Gap::Back, Gap::Streak];

    #[test]
    fn the_table_has_18_to_20_messages_without_duplicates_or_long_lines() {
        let t = table();
        assert_eq!(t.version, 1);
        assert!((18..=20).contains(&t.messages.len()), "{}", t.messages.len());
        let (mut ids, mut texts) = (HashSet::new(), HashSet::new());
        for m in &t.messages {
            assert!(ids.insert(m.id.clone()), "duplicate id {}", m.id);
            assert!(texts.insert(m.text.to_lowercase()), "duplicate text {}", m.text);
            assert!(m.text.chars().count() <= t.max_len, "too long: {}", m.text);
            assert!(m.text.is_ascii() && !m.text.trim().is_empty(), "plain English text: {}", m.text);
            for s in &m.time {
                assert!(["early", "morning", "noon", "afternoon", "evening", "night", "late"].contains(&s.as_str()), "{s}");
            }
            for s in &m.weekday {
                assert!(["monday", "friday", "weekend"].contains(&s.as_str()), "{s}");
            }
            for s in &m.gap {
                assert!(["first", "away", "back", "streak"].contains(&s.as_str()), "{s}");
            }
            if m.text.contains("{name}") {
                assert!(m.text.contains(", {name}"), "the name goes after a comma so it can be dropped: {}", m.text);
            }
            // the name even when the longest first name is used must fit the limit
            assert!(render(m, "Maximilian").chars().count() <= t.max_len + 10);
        }
    }

    #[test]
    fn every_hour_and_weekday_has_at_least_two_candidates() {
        let t = table();
        for hour in 0..24 {
            for wd in 0..7 {
                let n = candidates(t, hour, wd, Gap::None).len();
                assert!(n >= 2, "hour {hour} weekday {wd}: {n}");
            }
        }
    }

    #[test]
    fn every_special_gap_has_a_message_and_only_special_ones_are_returned() {
        let t = table();
        for g in GAPS {
            for hour in 0..24 {
                for wd in 0..7 {
                    let c = candidates(t, hour, wd, g);
                    assert!(!c.is_empty());
                    assert!(c.iter().all(|m| m.gap.iter().any(|x| x == g.as_str())));
                }
            }
        }
        // a message with a gap tag never shows up on an ordinary open
        assert!(candidates(t, 10, 2, Gap::None).iter().all(|m| m.gap.is_empty()));
    }

    #[test]
    fn the_name_is_used_in_some_messages_and_never_in_all_of_them() {
        let t = table();
        let named = t.messages.iter().filter(|m| m.text.contains("{name}")).count();
        assert!(named >= 4 && named * 2 <= t.messages.len(), "{named}");
        for hour in 0..24 {
            for wd in 0..7 {
                assert!(candidates(t, hour, wd, Gap::None).iter().any(|m| !m.text.contains("{name}")), "a nameless message for {hour} {wd}");
            }
        }
    }

    #[test]
    fn the_name_is_substituted_or_dropped_cleanly() {
        let t = table();
        let by = |id: &str| t.messages.iter().find(|m| m.id == id).unwrap();
        assert_eq!(render(by("morning-1"), "Zohaib"), "Good morning, Zohaib");
        assert_eq!(render(by("morning-1"), ""), "Good morning");
        assert_eq!(render(by("morning-1"), "  "), "Good morning");
        assert_eq!(render(by("late-1"), ""), "Still up?");
        assert_eq!(render(by("first-1"), ""), "Welcome. Let's make something.");
        assert_eq!(render(by("first-1"), "Ali"), "Welcome, Ali. Let's make something.");
        assert_eq!(render(by("neutral-1"), "Ali"), "Back at it");
        for m in &t.messages {
            assert!(!render(m, "").contains('{') && !render(m, "").contains(" ,") && !render(m, "").contains("  "), "{}", m.text);
            assert!(!render(m, "Ali").contains('{'));
        }
    }

    #[test]
    fn first_names_come_from_the_display_name_or_the_fallback() {
        assert_eq!(first_name("Zohaib Baig", "z@x.org"), "Zohaib");
        assert_eq!(first_name("", "ali@x.org"), "Ali");
        assert_eq!(first_name("  ", "ali"), "Ali");
        assert_eq!(first_name("", ""), "");
    }

    #[test]
    fn the_picker_never_repeats_the_last_message_and_follows_the_random_number() {
        let t = table();
        for hour in [0, 6, 10, 13, 16, 19, 22] {
            for wd in 0..7 {
                let c = candidates(t, hour, wd, Gap::None);
                for last in &c {
                    for i in 0..50 {
                        let m = pick(&c, Some(&last.id), i as f64 / 50.0).unwrap();
                        assert_ne!(m.id, last.id);
                    }
                }
            }
        }
        // a single candidate may repeat (there is nothing else)
        let one = [&t.messages[0]];
        assert_eq!(pick(&one, Some(&t.messages[0].id), 0.5).unwrap().id, t.messages[0].id);
        assert!(pick(&[], None, 0.5).is_none());
        // deterministic, and every candidate is reachable
        let c = candidates(t, 10, 2, Gap::None);
        assert_eq!(pick(&c, None, 0.3).unwrap().id, pick(&c, None, 0.3).unwrap().id);
        let seen: HashSet<_> = (0..100).map(|i| pick(&c, None, i as f64 / 100.0).unwrap().id.clone()).collect();
        assert_eq!(seen.len(), c.len());
        assert_eq!(pick(&c, None, 0.0).unwrap().id, c[0].id);
        assert_eq!(pick(&c, None, 1.0).unwrap().id, c.last().unwrap().id);
    }

    #[test]
    fn many_opens_in_a_row_never_repeat() {
        let t = table();
        let mut st = State::default();
        let mut x: u64 = 12345;
        for i in 0..400 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let rnd = (x >> 11) as f64 / (1u64 << 53) as f64;
            let m = Moment { hour: (i * 7 % 24) as u32, weekday: (i % 7) as u32, day: 20000 + i as i64 };
            let (text, next) = greet(t, &st, m, "Ali", rnd);
            assert!(!text.is_empty());
            let (gap, _) = visit(t, &st, m.day);
            if let (Some(a), Some(b)) = (&st.last_id, &next.last_id) {
                if candidates(t, m.hour, m.weekday, gap).len() > 1 {
                    assert_ne!(a, b, "open {i}");
                }
            }
            st = next;
        }
    }

    #[test]
    fn gaps_follow_the_days_since_the_last_open() {
        let t = table();
        let st = |last, streak| State { last_day: last, streak, last_id: None };
        assert_eq!(visit(t, &st(None, 0), 100), (Gap::First, 1));
        assert_eq!(visit(t, &st(Some(100), 3), 100), (Gap::None, 3)); // same day
        assert_eq!(visit(t, &st(Some(100), 1), 101), (Gap::None, 2));
        assert_eq!(visit(t, &st(Some(100), 4), 101), (Gap::Streak, 5));
        assert_eq!(visit(t, &st(Some(100), 9), 101), (Gap::Streak, 10));
        assert_eq!(visit(t, &st(Some(100), 9), 102), (Gap::None, 1)); // skipped a day: the streak is over
        assert_eq!(visit(t, &st(Some(100), 9), 103), (Gap::Back, 1));
        assert_eq!(visit(t, &st(Some(100), 9), 113), (Gap::Back, 1));
        assert_eq!(visit(t, &st(Some(100), 9), 114), (Gap::Away, 1));
        assert_eq!(visit(t, &st(Some(100), 9), 90), (Gap::None, 9)); // the clock went back
        // a second open on the same day does not use a gap message and keeps the day
        let m = Moment { hour: 10, weekday: 1, day: 100 };
        let (_, next) = greet(t, &st(Some(100), 6), m, "", 0.1);
        assert_eq!((next.last_day, next.streak), (Some(100), 6));
    }

    #[test]
    fn the_first_open_ever_gets_the_welcome() {
        let t = table();
        let (text, next) = greet(t, &State::default(), Moment { hour: 9, weekday: 2, day: 20000 }, "Ali", 0.4);
        assert_eq!(text, "Welcome, Ali. Let's make something.");
        assert_eq!((next.last_day, next.streak, next.last_id.as_deref()), (Some(20000), 1, Some("first-1")));
    }

    #[test]
    fn day_numbers_match_known_dates() {
        assert_eq!(day_number(1970, 1, 1), 0);
        assert_eq!(day_number(2000, 3, 1), 11017);
        assert_eq!(day_number(2026, 11, 14), 20771);
        assert_eq!(day_number(2024, 2, 29) + 1, day_number(2024, 3, 1));
        assert_eq!(day_number(2026, 12, 31) + 1, day_number(2027, 1, 1));
    }

    #[test]
    fn golden_cases_pass_here_and_are_the_shared_contract() {
        let t = table();
        assert!(t.golden.len() >= 8);
        for g in &t.golden {
            let wd = WEEKDAYS.iter().position(|w| *w == g.weekday).unwrap() as u32;
            let ids: Vec<&str> = candidates(t, g.hour, wd, Gap::parse(&g.gap)).iter().map(|m| m.id.as_str()).collect();
            assert_eq!(ids, g.ids, "{} {} {}", g.hour, g.weekday, g.gap);
        }
    }

    #[test]
    fn the_state_survives_a_round_trip_and_a_broken_file_is_fresh() {
        let dir = std::env::temp_dir().join(format!("zo-greet-{}", std::process::id()));
        let p = dir.join("greeting.json");
        assert_eq!(State::load(&p), State::default());
        let st = State { last_day: Some(5), streak: 2, last_id: Some("late-1".into()) };
        st.save(&p);
        assert_eq!(State::load(&p), st);
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(State::load(&p), State::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_table_is_safe_to_inline_in_a_script_tag() {
        assert!(!SOURCE.to_lowercase().contains("</script") && !SOURCE.contains("<!--"));
    }
}
