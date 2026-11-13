//! The question card (docs/INTERACTION.md section 4): what Zohara Agent shows when the model calls `zohara_ask_user`.
//! A header chip, the question, the options as radio rows (or check rows for a multi-select) with their descriptions, an
//! "Other" row with a text field, a "You decide" row (Zohara Agent picks the best option itself and says which), Skip, and for several questions Back / Next with one Send at the end.
//! Keys: 1 to 6 pick an option, D leaves it to Zohara Agent (optional), Enter goes on, Esc skips. The card checks nothing itself beyond "something is chosen": the
//! answer goes through `zo_core::question::validate_answers` before it is handed on.

use crate::{hbox, lbl, vbox};
use adw::prelude::*;
use gtk4 as gtk;
use gtk::glib;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use zo_core::approval::{Question, QuestionAnswer, QuestionResult};

/// One page: the buttons of the options, and the Other row.
struct Page {
    options: Vec<gtk::CheckButton>,
    other: gtk::CheckButton,
    other_text: gtk::Entry,
    /// "You decide": stands alone, so choosing it clears the rest and choosing anything else clears it.
    decide: gtk::CheckButton,
}

impl Page {
    /// What is chosen on this page (labels in the order they were offered, and the text of Other when it is on).
    fn answer(&self, q: &Question) -> QuestionAnswer {
        if self.decide.is_active() {
            return QuestionAnswer { selected: Vec::new(), other: None, decide: true };
        }
        let selected = q.options.iter().zip(&self.options).filter(|(_, b)| b.is_active()).map(|(o, _)| o.label.clone()).collect();
        let other = Some(self.other_text.text().to_string()).filter(|t| self.other.is_active() && !t.trim().is_empty());
        QuestionAnswer { selected, other, decide: false }
    }

    fn has_answer(&self, q: &Question) -> bool {
        let a = self.answer(q);
        a.decide || !a.selected.is_empty() || a.other.is_some()
    }
}

/// How the answer is told in the chat: "Red; Blue, my own words".
pub fn summary(questions: &[Question], r: &QuestionResult) -> String {
    match r {
        QuestionResult::Answers(a) => {
            let parts: Vec<String> = questions
                .iter()
                .zip(a)
                .map(|(_, a)| {
                    if a.decide {
                        return "you left it to Zohara Agent".to_string();
                    }
                    let mut v = a.selected.clone();
                    if let Some(o) = &a.other {
                        v.push(o.clone());
                    }
                    v.join(", ")
                })
                .collect();
            format!("You answered: {}", parts.join("; "))
        }
        QuestionResult::Declined => "You skipped the question.".into(),
        QuestionResult::Unanswered => "The question was not answered (it timed out or the answer was stopped).".into(),
    }
}

/// Builds the card. `done` is called once with what the person decided; when `expired` becomes true first (the wait ended:
/// timeout or Stop) it is called with `Unanswered` instead. The caller removes the card and writes the summary line.
pub fn build(questions: Vec<Question>, expired: Arc<AtomicBool>, done: impl Fn(QuestionResult) + 'static) -> gtk::Box {
    let questions = Rc::new(questions);
    let card = vbox(10);
    card.add_css_class("q-card");
    card.set_focusable(true);
    // screen readers: say what this is
    card.update_property(&[gtk::accessible::Property::Label("Zohara Agent is asking you a question")]);
    card.set_accessible_role(gtk::AccessibleRole::Group);

    let stack = gtk::Stack::new();
    let pages: Rc<RefCell<Vec<Page>>> = Rc::new(RefCell::new(Vec::new()));
    let cur = Rc::new(Cell::new(0usize));
    let finished = Rc::new(Cell::new(false));
    let done = Rc::new(done);

    let back = gtk::Button::with_label("Back");
    let next = gtk::Button::with_label("Next");
    next.add_css_class("suggested-action");
    let skip = gtk::Button::with_label("Skip");
    skip.add_css_class("flat");
    let step = lbl("", "q-step");
    step.set_hexpand(true);
    step.set_xalign(1.0);

    let n = questions.len();
    for (qi, q) in questions.iter().enumerate() {
        let page = vbox(8);
        let head = hbox(8);
        if !q.header.is_empty() {
            head.append(&lbl(&q.header, "q-chip"));
        }
        page.append(&head);
        let title = lbl(&q.question, "q-title");
        title.set_wrap(true);
        title.set_selectable(false);
        page.append(&title);

        let mut buttons: Vec<gtk::CheckButton> = Vec::new();
        for (oi, o) in q.options.iter().enumerate() {
            let b = gtk::CheckButton::new();
            if !q.multi_select {
                if let Some(first) = buttons.first() {
                    b.set_group(Some(first));
                }
            }
            let row = vbox(1);
            let l = lbl(&format!("{}  {}", oi + 1, o.label), "q-opt-label");
            l.set_wrap(true);
            row.append(&l);
            if !o.description.is_empty() {
                let d = lbl(&o.description, "q-opt-desc");
                d.set_wrap(true);
                row.append(&d);
            }
            b.set_child(Some(&row));
            b.add_css_class("q-opt");
            page.append(&b);
            buttons.push(b);
        }
        // Other: always there, with a text field
        let other = gtk::CheckButton::new();
        if !q.multi_select {
            if let Some(first) = buttons.first() {
                other.set_group(Some(first));
            }
        }
        let other_row = hbox(8);
        other_row.append(&lbl("Other", "q-opt-label"));
        let entry = gtk::Entry::new();
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some("Type your own answer"));
        entry.set_max_length(2000);
        other_row.append(&entry);
        other.set_child(Some(&other_row));
        other.add_css_class("q-opt");
        page.append(&other);
        // typing in the field picks Other
        {
            let other = other.clone();
            entry.connect_changed(move |e| {
                if !e.text().is_empty() && !other.is_active() {
                    other.set_active(true);
                }
            });
        }
        // You decide: Zohara Agent answers this one itself (D)
        let decide = gtk::CheckButton::new();
        let drow = vbox(1);
        let dl = lbl("D  You decide", "q-opt-label");
        drow.append(&dl);
        let dd = lbl("Zohara Agent picks the best option and tells you which and why.", "q-opt-desc");
        dd.set_wrap(true);
        drow.append(&dd);
        decide.set_child(Some(&drow));
        decide.add_css_class("q-opt");
        page.append(&decide);
        {
            // one or the other: "You decide" clears the choices; any choice clears "You decide"
            let guard = Rc::new(Cell::new(false));
            let all: Vec<gtk::CheckButton> = buttons.iter().cloned().chain(std::iter::once(other.clone())).collect();
            {
                let (guard, all, entry) = (guard.clone(), all.clone(), entry.clone());
                decide.connect_toggled(move |d| {
                    if d.is_active() && !guard.replace(true) {
                        for b in &all {
                            b.set_active(false);
                        }
                        entry.set_text("");
                        guard.set(false);
                    }
                });
            }
            for b in &all {
                let (guard, decide) = (guard.clone(), decide.clone());
                b.connect_toggled(move |b| {
                    if b.is_active() && !guard.replace(true) {
                        decide.set_active(false);
                        guard.set(false);
                    }
                });
            }
        }
        stack.add_named(&page, Some(&qi.to_string()));
        pages.borrow_mut().push(Page { options: buttons, other, other_text: entry, decide });
    }
    card.append(&stack);

    let foot = hbox(8);
    foot.append(&skip);
    foot.append(&step);
    foot.append(&back);
    foot.append(&next);
    card.append(&foot);

    // what the buttons say and whether they work, for the page that is showing
    let refresh: Rc<dyn Fn()> = {
        let (cur, pages, questions) = (cur.clone(), pages.clone(), questions.clone());
        let (stack, back, next, step) = (stack.clone(), back.clone(), next.clone(), step.clone());
        Rc::new(move || {
            let i = cur.get();
            stack.set_visible_child_name(&i.to_string());
            back.set_visible(i > 0);
            next.set_label(if i + 1 == n { "Send" } else { "Next" });
            step.set_text(&if n > 1 { format!("Question {} of {}", i + 1, n) } else { String::new() });
            next.set_sensitive(pages.borrow().get(i).is_some_and(|p| p.has_answer(&questions[i])));
        })
    };
    for p in pages.borrow().iter() {
        for b in p.options.iter().chain([&p.other, &p.decide]) {
            let r = refresh.clone();
            b.connect_toggled(move |_| r());
        }
        let r = refresh.clone();
        p.other_text.connect_changed(move |_| r());
    }
    refresh();

    let finish = {
        let (finished, done) = (finished.clone(), done.clone());
        Rc::new(move |r: QuestionResult| {
            if !finished.replace(true) {
                done(r);
            }
        })
    };
    {
        let finish = finish.clone();
        skip.connect_clicked(move |_| finish(QuestionResult::Declined));
    }
    {
        let (cur, refresh) = (cur.clone(), refresh.clone());
        back.connect_clicked(move |_| {
            cur.set(cur.get().saturating_sub(1));
            refresh();
        });
    }
    let go_next: Rc<dyn Fn()> = {
        let (cur, pages, questions, finish, refresh, next) = (cur.clone(), pages.clone(), questions.clone(), finish.clone(), refresh.clone(), next.clone());
        Rc::new(move || {
            if !next.is_sensitive() {
                return;
            }
            if cur.get() + 1 < n {
                cur.set(cur.get() + 1);
                refresh();
                return;
            }
            let raw = serde_json::Value::Array(pages.borrow().iter().zip(questions.iter()).map(|(p, q)| serde_json::to_value(p.answer(q)).unwrap_or_default()).collect());
            // the same check as everywhere else; a card that cannot make a valid answer says nothing and stays
            if let Ok(a) = zo_core::question::validate_answers(&questions, &raw) {
                finish(QuestionResult::Answers(a));
            }
        })
    };
    {
        let g = go_next.clone();
        next.connect_clicked(move |_| g());
    }
    for p in pages.borrow().iter() {
        let g = go_next.clone();
        p.other_text.connect_activate(move |_| g());
    }

    // keys: 1 to 6 pick an option, D = You decide (typing in the Other field is never taken), Enter goes on, Esc skips
    let keys = gtk::EventControllerKey::new();
    {
        let (cur, pages, finish, go_next, questions) = (cur.clone(), pages.clone(), finish.clone(), go_next.clone(), questions.clone());
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                finish(QuestionResult::Declined);
                return glib::Propagation::Stop;
            }
            if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
                go_next();
                return glib::Propagation::Stop;
            }
            if key == gtk::gdk::Key::d || key == gtk::gdk::Key::D {
                if let Some(p) = pages.borrow().get(cur.get()) {
                    p.decide.set_active(!p.decide.is_active());
                    return glib::Propagation::Stop;
                }
            }
            let digit = key.to_unicode().and_then(|c| c.to_digit(10)).map(|d| d as usize);
            if let Some(d) = digit.filter(|d| (1..=6).contains(d)) {
                if let Some(b) = pages.borrow().get(cur.get()).and_then(|p| p.options.get(d - 1)) {
                    if questions[cur.get()].multi_select {
                        b.set_active(!b.is_active());
                    } else {
                        b.set_active(true);
                    }
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
    }
    card.add_controller(keys);

    // the wait ended without the person (timeout, Stop): the card says so and closes
    {
        let finish = finish.clone();
        glib::timeout_add_local(Duration::from_millis(500), move || {
            if finished_flag(&expired) {
                finish(QuestionResult::Unanswered);
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }
    card.grab_focus();
    card
}

fn finished_flag(f: &AtomicBool) -> bool {
    f.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zo_core::approval::QuestionOption;

    fn q(multi: bool) -> Question {
        Question { question: "Which?".into(), header: "H".into(), multi_select: multi, options: vec![QuestionOption { label: "a".into(), description: String::new() }, QuestionOption { label: "b".into(), description: String::new() }] }
    }

    #[test]
    fn the_chat_line_says_what_was_decided() {
        let qs = vec![q(true), q(false), q(false)];
        let a = QuestionResult::Answers(vec![QuestionAnswer { selected: vec!["a".into(), "b".into()], other: Some("c".into()), decide: false }, QuestionAnswer { selected: vec![], other: Some("mine".into()), decide: false }, QuestionAnswer { selected: vec![], other: None, decide: true }]);
        assert_eq!(summary(&qs, &a), "You answered: a, b, c; mine; you left it to Zohara Agent");
        assert_eq!(summary(&qs, &QuestionResult::Declined), "You skipped the question.");
        assert!(summary(&qs, &QuestionResult::Unanswered).contains("not answered"));
    }

    fn all<T: IsA<gtk::Widget>>(w: &gtk::Widget, out: &mut Vec<T>) {
        if let Some(t) = w.downcast_ref::<T>() {
            out.push(t.clone());
        }
        let mut c = w.first_child();
        while let Some(x) = c {
            all(&x, out);
            c = x.next_sibling();
        }
    }
    fn of<T: IsA<gtk::Widget>>(card: &gtk::Box) -> Vec<T> {
        let mut v = Vec::new();
        all(card.upcast_ref(), &mut v);
        v
    }
    fn button(card: &gtk::Box, label: &str) -> gtk::Button {
        of::<gtk::Button>(card).into_iter().find(|b| b.label().as_deref() == Some(label)).unwrap_or_else(|| panic!("no {label} button"))
    }

    /// Run with a display: `xvfb-run -a cargo test -p zohara-agent -- --ignored the_card`
    #[test]
    #[ignore = "needs a display (run under xvfb)"]
    fn the_card_collects_checks_and_ends_the_way_the_person_decides() {
        gtk::init().expect("a display");
        let make = |qs: Vec<Question>, expired: Arc<AtomicBool>| {
            let got: Rc<RefCell<Vec<QuestionResult>>> = Rc::default();
            let g2 = got.clone();
            (build(qs, expired, move |r| g2.borrow_mut().push(r)), got)
        };
        // two questions: a multi-select and a single-select
        let (card, got) = make(vec![q(true), q(false)], Arc::new(AtomicBool::new(false)));
        let checks = of::<gtk::CheckButton>(&card);
        assert_eq!(checks.len(), 8, "two options, Other and You decide for each question");
        let next = button(&card, "Next");
        assert!(!next.is_sensitive(), "nothing chosen yet");
        checks[0].set_active(true);
        checks[1].set_active(true);
        assert!(next.is_sensitive());
        next.emit_clicked();
        assert_eq!(next.label().as_deref(), Some("Send"), "the last page sends");
        assert!(!next.is_sensitive(), "the second question has no answer yet");
        assert!(got.borrow().is_empty());
        // typing in the Other field picks Other and allows sending
        let entries = of::<gtk::Entry>(&card);
        entries[1].set_text("my own words");
        assert!(checks[6].is_active() && next.is_sensitive());
        // Back keeps the first page's choices
        button(&card, "Back").emit_clicked();
        assert!(checks[0].is_active() && checks[1].is_active());
        next.emit_clicked();
        next.emit_clicked();
        assert_eq!(*got.borrow(), vec![QuestionResult::Answers(vec![
            QuestionAnswer { selected: vec!["a".into(), "b".into()], other: None, decide: false },
            QuestionAnswer { selected: vec![], other: Some("my own words".into()), decide: false },
        ])]);
        next.emit_clicked();
        assert_eq!(got.borrow().len(), 1, "the answer is given once");

        // a single-select question allows one choice
        let (card, got) = make(vec![q(false)], Arc::new(AtomicBool::new(false)));
        let checks = of::<gtk::CheckButton>(&card);
        checks[0].set_active(true);
        checks[1].set_active(true);
        assert!(!checks[0].is_active() && checks[1].is_active());
        button(&card, "Send").emit_clicked();
        assert_eq!(*got.borrow(), vec![QuestionResult::Answers(vec![QuestionAnswer { selected: vec!["b".into()], other: None, decide: false }])]);

        // You decide: stands alone, in a single-select and in a multi-select question
        let (card, got) = make(vec![q(true), q(false)], Arc::new(AtomicBool::new(false)));
        let checks = of::<gtk::CheckButton>(&card);
        let next = button(&card, "Next");
        checks[0].set_active(true);
        checks[1].set_active(true);
        checks[3].set_active(true); // You decide clears the choices
        assert!(!checks[0].is_active() && !checks[1].is_active() && checks[3].is_active());
        assert!(next.is_sensitive(), "You decide is an answer");
        checks[0].set_active(true); // a choice clears You decide
        assert!(!checks[3].is_active() && checks[0].is_active());
        checks[3].set_active(true);
        next.emit_clicked();
        checks[4].set_active(true); // second question: pick b then change to You decide
        checks[7].set_active(true);
        assert!(!checks[4].is_active());
        next.emit_clicked();
        assert_eq!(*got.borrow(), vec![QuestionResult::Answers(vec![
            QuestionAnswer { selected: vec![], other: None, decide: true },
            QuestionAnswer { selected: vec![], other: None, decide: true },
        ])]);

        // Skip
        let (card, got) = make(vec![q(false)], Arc::new(AtomicBool::new(false)));
        button(&card, "Skip").emit_clicked();
        assert_eq!(*got.borrow(), vec![QuestionResult::Declined]);

        // the wait ended without the person: the card closes itself as "not answered"
        let flag = Arc::new(AtomicBool::new(false));
        let (_card, got) = make(vec![q(false)], flag.clone());
        flag.store(true, Ordering::SeqCst);
        let ctx = glib::MainContext::default();
        let t = std::time::Instant::now();
        while got.borrow().is_empty() && t.elapsed() < Duration::from_secs(3) {
            ctx.iteration(false);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*got.borrow(), vec![QuestionResult::Unanswered]);
    }
}
