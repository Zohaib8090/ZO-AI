//! The question tool (`zohara_ask_user`), the pure parts: checking what the model asks, checking what the person answers,
//! the text the model gets back, and the table of questions that are open right now. The contract is docs/INTERACTION.md.
//!
//! Nothing here talks to a client: `service.rs` (phone, web link, cloud), `zohara-agent` (the desktop window) and the tests each bring
//! their own `Questioner`; they all use these functions, so the rules are the same everywhere.

use crate::approval::{Question, QuestionAnswer, QuestionOption, QuestionResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_QUESTIONS: usize = 4;
pub const MAX_QUESTION_CHARS: usize = 300;
pub const MAX_HEADER_CHARS: usize = 24;
pub const MAX_LABEL_CHARS: usize = 60;
pub const MAX_DESC_CHARS: usize = 200;
pub const MAX_OPTIONS: usize = 6;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OTHER_CHARS: usize = 2000;
/// How long a question waits for the person (INTERACTION.md section 2).
pub const WAIT: Duration = Duration::from_secs(600);

/// Control characters (newlines and tabs become a space) removed, the ends trimmed, cut to `max` characters.
pub fn clean(s: &str, max: usize) -> String {
    let one: String = s.chars().map(|c| if c == '\n' || c == '\t' || c == '\r' { ' ' } else { c }).filter(|c| !c.is_control()).collect();
    one.trim().chars().take(max).collect()
}

/// The model's `questions` argument, checked. Texts are cleaned (the client shows them as text, never as markup).
pub fn parse_questions(args: &Value) -> Result<Vec<Question>, String> {
    let qs = args.get("questions").and_then(Value::as_array).ok_or("Missing or invalid 'questions' parameter.")?;
    if qs.is_empty() || qs.len() > MAX_QUESTIONS {
        return Err(format!("Ask 1 to {MAX_QUESTIONS} questions."));
    }
    let mut out = Vec::new();
    for q in qs {
        let raw_q = q.get("question").and_then(Value::as_str).unwrap_or("");
        if raw_q.trim().is_empty() || raw_q.chars().count() > MAX_QUESTION_CHARS {
            return Err(format!("Each question must be 1 to {MAX_QUESTION_CHARS} characters."));
        }
        let raw_h = q.get("header").and_then(Value::as_str).unwrap_or("");
        if raw_h.chars().count() > MAX_HEADER_CHARS {
            return Err(format!("Each header must be at most {MAX_HEADER_CHARS} characters."));
        }
        let opts = q.get("options").and_then(Value::as_array).ok_or("Each question needs an 'options' array.")?;
        if opts.len() < MIN_OPTIONS || opts.len() > MAX_OPTIONS {
            return Err(format!("Each question needs {MIN_OPTIONS} to {MAX_OPTIONS} options."));
        }
        let mut options: Vec<QuestionOption> = Vec::new();
        for o in opts {
            let raw_l = o.get("label").and_then(Value::as_str).unwrap_or("");
            let raw_d = o.get("description").and_then(Value::as_str).unwrap_or("");
            let label = clean(raw_l, MAX_LABEL_CHARS + 1);
            if label.is_empty() || raw_l.chars().count() > MAX_LABEL_CHARS {
                return Err(format!("Each option label must be 1 to {MAX_LABEL_CHARS} characters."));
            }
            if raw_d.chars().count() > MAX_DESC_CHARS {
                return Err(format!("Each option description must be at most {MAX_DESC_CHARS} characters."));
            }
            if label.eq_ignore_ascii_case("other") {
                return Err("Do not add an 'Other' option: the app adds it.".into());
            }
            if options.iter().any(|x| x.label == label) {
                return Err("Option labels must be different from each other.".into());
            }
            options.push(QuestionOption { label, description: clean(raw_d, MAX_DESC_CHARS) });
        }
        out.push(Question { question: clean(raw_q, MAX_QUESTION_CHARS), header: clean(raw_h, MAX_HEADER_CHARS), multi_select: q.get("multi_select").and_then(Value::as_bool).unwrap_or(false), options });
    }
    Ok(out)
}

/// The person's `answers` array against the questions that were asked: same length, every chosen label one of the offered ones
/// (each at most once), one choice only for a single-select question, `other` cut to 2000 characters without control characters,
/// and something chosen or written for every question. Anything else is refused and the question stays open.
pub fn validate_answers(questions: &[Question], raw: &Value) -> Result<Vec<QuestionAnswer>, String> {
    let list = raw.as_array().ok_or("The answers are not a list.")?;
    if list.len() != questions.len() {
        return Err("There must be one answer for every question.".into());
    }
    let mut out = Vec::new();
    for (q, a) in questions.iter().zip(list) {
        let mut selected: Vec<String> = Vec::new();
        match a.get("selected") {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                for it in items {
                    let label = it.as_str().ok_or("A chosen option is not text.")?;
                    if !q.options.iter().any(|o| o.label == label) {
                        return Err("A chosen option was not offered.".into());
                    }
                    if selected.iter().any(|s| s == label) {
                        return Err("An option was chosen twice.".into());
                    }
                    selected.push(label.to_string());
                }
            }
            Some(_) => return Err("The chosen options are not a list.".into()),
        }
        let other = match a.get("other") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(clean(s, MAX_OTHER_CHARS)).filter(|s| !s.is_empty()),
            Some(_) => return Err("The other answer is not text.".into()),
        };
        if !q.multi_select && selected.len() + usize::from(other.is_some()) > 1 {
            return Err("Only one answer is allowed for this question.".into());
        }
        if selected.is_empty() && other.is_none() {
            return Err("Every question needs an answer (or skip them all).".into());
        }
        out.push(QuestionAnswer { selected, other });
    }
    Ok(out)
}

/// What the model is told: `Q: ...\nA: label, label [Other: text]` per question, or why there is no answer.
pub fn format_result(questions: &[Question], r: &QuestionResult) -> String {
    match r {
        QuestionResult::Answers(answers) => questions
            .iter()
            .zip(answers)
            .map(|(q, a)| {
                let mut s = format!("Q: {}\nA: {}", q.question, a.selected.join(", "));
                if let Some(o) = a.other.as_deref().filter(|o| !o.is_empty()) {
                    s.push_str(&format!("{}[Other: {o}]", if a.selected.is_empty() { "" } else { " " }));
                }
                s
            })
            .collect::<Vec<_>>()
            .join("\n"),
        QuestionResult::Declined => "The person declined to answer.".into(),
        QuestionResult::Unanswered => "The person did not answer.".into(),
    }
}

/// The questions that are open on one connection. `reply` takes a client's `{"op":"answer", ...}` line.
#[derive(Default)]
pub struct Pending {
    map: Mutex<HashMap<u64, (Vec<Question>, mpsc::Sender<QuestionResult>)>>,
}

impl Pending {
    pub fn open(&self, id: u64, questions: Vec<Question>) -> mpsc::Receiver<QuestionResult> {
        let (tx, rx) = mpsc::channel();
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(id, (questions, tx));
        rx
    }

    pub fn close(&self, id: u64) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    }

    pub fn is_open(&self, id: u64) -> bool {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id)
    }

    /// Nobody is left to answer: every waiting question ends as unanswered.
    pub fn clear(&self) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// An answer line from a client. True when it was for an open question and was valid (the question is then closed: a second
    /// answer finds nothing). Unknown ids and invalid answers change nothing.
    pub fn reply(&self, v: &Value) -> bool {
        let Some(id) = v.get("id").and_then(Value::as_u64) else { return false };
        let mut g = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let Some((questions, _)) = g.get(&id) else { return false };
        let result = if v.get("cancelled").and_then(Value::as_bool) == Some(true) {
            QuestionResult::Declined
        } else {
            match v.get("answers").map(|a| validate_answers(questions, a)) {
                Some(Ok(a)) => QuestionResult::Answers(a),
                _ => return false,
            }
        };
        let Some((_, tx)) = g.remove(&id) else { return false };
        tx.send(result).is_ok()
    }
}

/// Waits for the person: until an answer, `limit`, the run is stopped (checked every 200 ms) or every sender is gone.
pub fn wait(rx: &mpsc::Receiver<QuestionResult>, cancel: &AtomicBool, limit: Duration) -> QuestionResult {
    let end = Instant::now() + limit;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return QuestionResult::Unanswered;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return QuestionResult::Unanswered;
        }
        match rx.recv_timeout(left.min(Duration::from_millis(200))) {
            Ok(r) => return r,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return QuestionResult::Unanswered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one() -> Value {
        json!({"questions": [{"question": "Which database?", "header": "Database", "multi_select": false,
            "options": [{"label": "PostgreSQL", "description": "Relational"}, {"label": "SQLite", "description": "A file"}]}]})
    }
    fn two_multi() -> Vec<Question> {
        parse_questions(&json!({"questions": [
            {"question": "A?", "header": "A", "multi_select": true, "options": [{"label": "x", "description": ""}, {"label": "y", "description": ""}, {"label": "z", "description": ""}]},
            {"question": "B?", "header": "B", "multi_select": false, "options": [{"label": "p", "description": ""}, {"label": "q", "description": ""}]}]})).unwrap()
    }

    #[test]
    fn the_models_questions_are_checked() {
        let q = parse_questions(&one()).unwrap();
        assert_eq!((q.len(), q[0].options.len(), q[0].header.as_str()), (1, 2, "Database"));
        let bad = |v: Value| parse_questions(&v).unwrap_err();
        assert!(bad(json!({})).contains("questions"));
        assert!(bad(json!({"questions": []})).contains("1 to 4"));
        let five: Vec<Value> = (0..5).map(|_| one()["questions"][0].clone()).collect();
        assert!(bad(json!({"questions": five})).contains("1 to 4"));
        let mut q1 = one();
        q1["questions"][0]["options"] = json!([{"label": "Only", "description": ""}]);
        assert!(bad(q1).contains("2 to 6"));
        let mut q2 = one();
        q2["questions"][0]["question"] = json!("x".repeat(301));
        assert!(bad(q2).contains("300"));
        let mut q3 = one();
        q3["questions"][0]["header"] = json!("h".repeat(25));
        assert!(bad(q3).contains("24"));
        let mut q4 = one();
        q4["questions"][0]["options"][0]["label"] = json!("l".repeat(61));
        assert!(bad(q4).contains("60"));
        let mut q5 = one();
        q5["questions"][0]["options"][1]["label"] = json!("PostgreSQL");
        assert!(bad(q5).contains("different"), "labels must be unique: the answer check relies on it");
        let mut q6 = one();
        q6["questions"][0]["options"][1]["label"] = json!("other");
        assert!(bad(q6).contains("Other"));
        let mut q7 = one();
        q7["questions"][0]["options"][1]["description"] = json!("d".repeat(201));
        assert!(bad(q7).contains("200"));
        // control characters never reach the client
        let mut q8 = one();
        q8["questions"][0]["question"] = json!("Which\u{7} one?\n");
        assert_eq!(parse_questions(&q8).unwrap()[0].question, "Which one?");
    }

    #[test]
    fn the_persons_answers_are_checked_against_what_was_asked() {
        let qs = two_multi();
        let ok = validate_answers(&qs, &json!([{"selected": ["x", "z"], "other": null}, {"selected": [], "other": "  my own\u{0}\n answer "}])).unwrap();
        assert_eq!(ok[0].selected, vec!["x", "z"]);
        assert_eq!(ok[1].other.as_deref(), Some("my own  answer"));
        let no = |v: Value| validate_answers(&qs, &v).unwrap_err();
        assert!(no(json!([{"selected": ["x"]}])).contains("one answer for every"), "too few");
        assert!(no(json!([{"selected": ["x"]}, {"selected": ["p"]}, {"selected": ["p"]}])).contains("one answer for every"), "too many");
        assert!(no(json!([{"selected": ["nope"]}, {"selected": ["p"]}])).contains("not offered"), "a label that was never offered");
        assert!(no(json!([{"selected": ["x", "x"]}, {"selected": ["p"]}])).contains("twice"));
        assert!(no(json!([{"selected": ["x"]}, {"selected": ["p", "q"]}])).contains("Only one"), "two for a single-select question");
        assert!(no(json!([{"selected": ["x"]}, {"selected": ["p"], "other": "also"}])).contains("Only one"), "a choice and Other on a single-select");
        assert!(no(json!([{"selected": []}, {"selected": ["p"]}])).contains("needs an answer"));
        assert!(no(json!([{"selected": [3]}, {"selected": ["p"]}])).contains("not text"));
        assert!(no(json!({"selected": ["x"]})).contains("not a list"));
        // the free text is cut at 2000 characters
        let long = validate_answers(&qs, &json!([{"other": "é".repeat(5000)}, {"selected": ["p"]}])).unwrap();
        assert_eq!(long[0].other.as_ref().unwrap().chars().count(), 2000);
    }

    #[test]
    fn the_model_gets_plain_text() {
        let qs = two_multi();
        let a = QuestionResult::Answers(vec![QuestionAnswer { selected: vec!["x".into(), "y".into()], other: Some("and w".into()) }, QuestionAnswer { selected: vec![], other: Some("custom".into()) }]);
        assert_eq!(format_result(&qs, &a), "Q: A?\nA: x, y [Other: and w]\nQ: B?\nA: [Other: custom]");
        assert_eq!(format_result(&qs, &QuestionResult::Declined), "The person declined to answer.");
        assert_eq!(format_result(&qs, &QuestionResult::Unanswered), "The person did not answer.");
    }

    #[test]
    fn an_open_question_takes_one_valid_answer_and_nothing_else() {
        let p = Pending::default();
        let rx = p.open(5, two_multi());
        assert!(!p.reply(&json!({"op": "answer", "id": 6, "cancelled": true})), "an id that is not open is ignored");
        assert!(!p.reply(&json!({"op": "answer", "id": 5, "answers": [{"selected": ["nope"]}, {"selected": ["p"]}]})), "an invalid answer is ignored");
        assert!(p.is_open(5) && rx.try_recv().is_err(), "the question is still open");
        assert!(!p.reply(&json!({"op": "answer", "id": 5, "cancelled": "yes"})), "cancelled must be true");
        assert!(p.reply(&json!({"op": "answer", "id": 5, "answers": [{"selected": ["y"]}, {"selected": ["q"]}]})));
        match rx.try_recv().unwrap() {
            QuestionResult::Answers(a) => assert_eq!((a[0].selected.clone(), a[1].selected.clone()), (vec!["y".to_string()], vec!["q".to_string()])),
            other => panic!("{other:?}"),
        }
        assert!(!p.reply(&json!({"op": "answer", "id": 5, "cancelled": true})), "a second answer finds nothing");
        let rx2 = p.open(7, two_multi());
        assert!(p.reply(&json!({"op": "answer", "id": 7, "cancelled": true})));
        assert_eq!(rx2.try_recv().unwrap(), QuestionResult::Declined);
        let rx3 = p.open(8, two_multi());
        p.clear();
        assert_eq!(wait(&rx3, &AtomicBool::new(false), Duration::from_secs(5)), QuestionResult::Unanswered, "a client that left ends the wait at once");
    }

    #[test]
    fn waiting_ends_on_stop_and_on_the_limit() {
        let p = Pending::default();
        let rx = p.open(1, two_multi());
        let t = Instant::now();
        assert_eq!(wait(&rx, &AtomicBool::new(true), Duration::from_secs(30)), QuestionResult::Unanswered);
        assert!(t.elapsed() < Duration::from_secs(1), "Stop does not wait for the 10 minutes");
        let t = Instant::now();
        assert_eq!(wait(&rx, &AtomicBool::new(false), Duration::from_millis(300)), QuestionResult::Unanswered);
        assert!(t.elapsed() >= Duration::from_millis(300) && t.elapsed() < Duration::from_secs(2));
    }
}
