//! The live step counter ("Step 7") and the "Repeating the same step" notice of a running answer.
//!
//! The agent loop sends `Steps(n)` on every tool start and `Stuck { n, tool }` when it saw the same call over and over (see
//! `zo_core::ai::agent::StepWatch`). The loop never stops anything by itself; this notice only offers Stop to the person.

use crate::{hbox, lbl, secondary};
use gtk4 as gtk;
use gtk::prelude::*;

pub const STUCK_TEXT: &str = "Repeating the same step. Stop?";

/// What the counter says. Nothing before the first tool call.
pub fn step_label(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("Step {n}")
    }
}

/// The tooltip of the counter.
pub fn step_tip(n: usize) -> String {
    format!("{n} tool call{} so far in this answer", if n == 1 { "" } else { "s" })
}

/// Decides whether a `stuck` event shows the notice: once per answer, and again only after 10 more calls than when it was last
/// shown (the core already does this; the screen does not trust a repeat to be wanted, e.g. after a relay resend).
#[derive(Default)]
pub struct StuckGate {
    last: Option<usize>,
}

impl StuckGate {
    pub fn should_show(&mut self, n: usize) -> bool {
        match self.last {
            Some(prev) if n < prev + 10 => false,
            _ => {
                self.last = Some(n);
                true
            }
        }
    }
}

/// The notice: one line and a Stop button. `on_stop` runs when Stop is pressed; the button then says "Stopping…".
pub fn notice(on_stop: impl Fn() + 'static) -> gtk::Box {
    let b = hbox(10);
    b.add_css_class("stuck-notice");
    let l = lbl(STUCK_TEXT, "stuck-text");
    l.set_hexpand(true);
    l.set_xalign(0.0);
    l.set_wrap(true);
    let stop = secondary("Stop");
    stop.add_css_class("danger");
    stop.set_valign(gtk::Align::Center);
    stop.connect_clicked(move |btn| {
        on_stop();
        btn.set_sensitive(false);
        btn.set_label("Stopping…");
    });
    b.append(&l);
    b.append(&stop);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_reads_step_n() {
        assert_eq!(step_label(0), "");
        assert_eq!(step_label(1), "Step 1");
        assert_eq!(step_label(7), "Step 7");
        assert_eq!(step_tip(1), "1 tool call so far in this answer");
        assert_eq!(step_tip(7), "7 tool calls so far in this answer");
    }

    #[test]
    fn the_notice_shows_once_and_again_only_after_ten_more() {
        let mut g = StuckGate::default();
        assert!(g.should_show(8));
        assert!(!g.should_show(8), "a repeat of the same event");
        assert!(!g.should_show(17));
        assert!(g.should_show(18));
        assert!(!g.should_show(19));
    }

    #[test]
    fn the_notice_text_is_the_agreed_one() {
        assert_eq!(STUCK_TEXT, "Repeating the same step. Stop?");
    }

    /// Run with a display: `xvfb-run -a cargo test -p zohara-agent -- --ignored the_notice_card`
    #[test]
    #[ignore = "needs a display (run under xvfb)"]
    fn the_notice_card_offers_stop_and_stops_only_when_pressed() {
        use std::cell::Cell;
        use std::rc::Rc;
        gtk::init().expect("a display");
        let stopped = Rc::new(Cell::new(0));
        let s2 = stopped.clone();
        let card = notice(move || s2.set(s2.get() + 1));
        let mut kids = Vec::new();
        let mut c = card.first_child();
        while let Some(x) = c {
            kids.push(x.clone());
            c = x.next_sibling();
        }
        let label = kids[0].downcast_ref::<gtk::Label>().expect("text first");
        assert_eq!(label.text(), STUCK_TEXT);
        let btn = kids[1].downcast_ref::<gtk::Button>().expect("button second");
        assert_eq!(btn.label().as_deref(), Some("Stop"));
        assert_eq!(stopped.get(), 0, "showing the notice stops nothing");
        btn.emit_clicked();
        assert_eq!(stopped.get(), 1);
        assert!(!btn.is_sensitive());
        assert_eq!(btn.label().as_deref(), Some("Stopping…"));
    }
}
