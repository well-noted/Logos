//! One undo history per model (spec part I, I3): whole-file states before each edit to the
//! code, the Info text, the interface or the experiments. Typing in the code or the Info text
//! joins one step until it pauses.

use std::time::{Duration, Instant};

/// Typing that pauses longer than this starts a new undo step.
pub const TYPING_PAUSE: Duration = Duration::from_millis(1500);
/// Undo steps kept per model.
const LIMIT: usize = 200;

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<String>,
    redo: Vec<String>,
    typing: Option<(&'static str, Instant)>,
}

impl History {
    /// Records the file text before an edit. `typing` names a text being typed (`code`,
    /// `info`); typing of the same kind within [`TYPING_PAUSE`] joins the previous step.
    pub fn record(&mut self, before: String, typing: Option<&'static str>, now: Instant) {
        let continues = matches!((self.typing, typing),
            (Some((k, at)), Some(t)) if k == t && now.duration_since(at) < TYPING_PAUSE);
        if !continues {
            self.undo.push(before);
            if self.undo.len() > LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.typing = typing.map(|t| (t, now));
    }

    /// The text to go back to, given the current one; None when there is nothing to undo.
    pub fn undo(&mut self, current: String) -> Option<String> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        self.typing = None;
        Some(prev)
    }

    pub fn redo(&mut self, current: String) -> Option<String> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.typing = None;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        *self = History::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos_nlogox::Nlogox;
    use logos_nlogox::widgets::new_widget_xml;

    const AA4: &str = include_str!("../../../corpus/own/aa4-combined/AA4-Combined.nlogox");

    /// I3: twenty mixed code, Info, interface and experiment edits, undone, give the file
    /// back byte for byte; redone, they give the edited file back.
    #[test]
    fn twenty_mixed_edits_undo_to_the_original_bytes() {
        let mut n = Nlogox::parse(AA4).unwrap();
        let mut h = History::default();
        let mut t = Instant::now();
        let step = |t: &mut Instant| {
            *t += TYPING_PAUSE * 2;
            *t
        };
        for i in 0..5 {
            // Code typed in two bursts close together: one step.
            let before = n.text().to_string();
            let code = n.code().unwrap();
            n.set_code(&format!("{code}\n; edit {i}")).unwrap();
            h.record(before, Some("code"), step(&mut t));
            let before = n.text().to_string();
            let code = n.code().unwrap();
            n.set_code(&format!("{code}!")).unwrap();
            h.record(before, Some("code"), t + Duration::from_millis(100));
            // Info.
            let before = n.text().to_string();
            let info = n.info().unwrap();
            n.set_info(&format!("{info}\nmore {i}")).unwrap();
            h.record(before, Some("info"), step(&mut t));
            // Interface: move and add.
            let before = n.text().to_string();
            let w = n.typed_widget(1).unwrap();
            n.set_widget_bounds(1, w.x + 5, w.y + 5, w.width, w.height)
                .unwrap();
            h.record(before, None, step(&mut t));
            let before = n.text().to_string();
            n.add_widget(&new_widget_xml("note", 10, 900 + i * 40, "x").unwrap())
                .unwrap();
            h.record(before, None, step(&mut t));
            // Experiment.
            let before = n.text().to_string();
            n.set_experiment_attribute(0, "repetitions", &format!("{}", i + 2))
                .unwrap();
            h.record(before, None, step(&mut t));
        }
        let edited = n.text().to_string();
        let mut steps = 0;
        while let Some(prev) = h.undo(n.text().to_string()) {
            n = Nlogox::parse(prev).unwrap();
            steps += 1;
        }
        assert_eq!(steps, 25, "5 rounds of 5 steps; the two code bursts joined");
        assert_eq!(n.text(), AA4);
        while let Some(next) = h.redo(n.text().to_string()) {
            n = Nlogox::parse(next).unwrap();
        }
        assert_eq!(n.text(), edited);
    }

    #[test]
    fn a_new_edit_drops_redo() {
        let mut h = History::default();
        let t = Instant::now();
        h.record("a".into(), None, t);
        assert_eq!(h.undo("b".into()).as_deref(), Some("a"));
        assert!(h.can_redo());
        h.record("a".into(), Some("code"), t);
        assert!(!h.can_redo());
    }
}
