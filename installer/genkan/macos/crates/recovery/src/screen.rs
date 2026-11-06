// SPDX-License-Identifier: GPL-3.0-or-later
//! What Finish Installing Sekka's window shows, apart from AppKit: its
//! screens, when it may close, and the output behind Show Details, kept so
//! that the view is only ever sent what is new.
use std::collections::VecDeque;

use crate::finish::Phase;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Checking,
    Broken(String),
    WrongRecovery,
    NotOneTrue,
    Ready { error: Option<String> },
    Working(Phase),
    Done,
}

impl Screen {
    /// Whether `bputil`, `kmutil` or `bless` may be running. Each answers
    /// to the app's pseudo-terminal, and one stopped part way can leave the
    /// startup policy or the boot object half changed, so while a step runs
    /// the window stays open and the app doesn't quit.
    pub fn busy(&self) -> bool {
        matches!(self, Screen::Working(_))
    }
}

/// Said when someone tries to quit or close the window while a step runs.
pub const BUSY: &str = "Wait for this step to finish";
pub const BUSY_DETAIL: &str = "macOS is changing how your Mac starts up, and stopping it part way isn't safe. Finish Installing Sekka can quit when this step is done.";

/// How many pieces of output Show Details keeps.
pub const LOG_LIMIT: usize = 2000;

/// The tools' output as it arrived, redacted, keeping the latest
/// [`LOG_LIMIT`] pieces.
#[derive(Debug, Default)]
pub struct Log {
    pieces: VecDeque<String>,
    /// How many pieces have been let go from the front.
    dropped: u64,
}

impl Log {
    pub fn push(&mut self, piece: String) {
        if self.pieces.len() == LOG_LIMIT {
            self.pieces.pop_front();
            self.dropped += 1;
        }
        self.pieces.push_back(piece);
    }

    pub fn text(&self) -> String {
        self.pieces.iter().map(String::as_str).collect()
    }
}

/// What brings a view of the log up to date: delete `trim` UTF-16 units
/// (AppKit's characters) from its start, then add `append` at its end.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Edit {
    pub trim: usize,
    pub append: String,
}

/// Which pieces of the log a view shows, so that catching up touches only
/// what changed: what is selected stays selected, and nothing is laid out
/// again when nothing arrived.
#[derive(Debug, Default)]
pub struct Shown {
    /// The log's index of the first piece shown.
    first: u64,
    /// Each piece shown, by its length in UTF-16 units.
    lengths: VecDeque<usize>,
}

impl Shown {
    /// The edit that makes the view show `log`, or `None` when it already
    /// does.
    pub fn catch_up(&mut self, log: &Log) -> Option<Edit> {
        let mut trim = 0;
        while self.first < log.dropped {
            let Some(length) = self.lengths.pop_front() else { break };
            trim += length;
            self.first += 1;
        }
        let next = (self.first + self.lengths.len() as u64).max(log.dropped);
        if self.lengths.is_empty() {
            self.first = next;
        }
        let mut append = String::new();
        for piece in log.pieces.iter().skip((next - log.dropped) as usize) {
            self.lengths.push_back(piece.encode_utf16().count());
            append.push_str(piece);
        }
        (trim > 0 || !append.is_empty()).then_some(Edit { trim, append })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view as AppKit holds it, edited as the window edits it.
    fn apply(view: &mut Vec<u16>, edit: &Edit) {
        view.drain(..edit.trim);
        view.extend(edit.append.encode_utf16());
    }

    #[test]
    fn only_a_running_step_keeps_the_app_open() {
        assert!(Screen::Working(Phase::Allowing).busy());
        assert!(Screen::Working(Phase::Selecting).busy());
        for screen in [
            Screen::Checking,
            Screen::Broken("x".into()),
            Screen::WrongRecovery,
            Screen::NotOneTrue,
            Screen::Ready { error: None },
            Screen::Done,
        ] {
            assert!(!screen.busy(), "{screen:?}");
        }
    }

    #[test]
    fn the_view_is_sent_only_what_is_new() {
        let mut log = Log::default();
        let mut shown = Shown::default();
        let mut view = Vec::new();
        assert_eq!(shown.catch_up(&log), None);
        log.push("Authorized user: ".into());
        log.push("Password: [redacted]\r\n".into());
        let edit = shown.catch_up(&log).unwrap();
        assert_eq!(edit.trim, 0);
        apply(&mut view, &edit);
        assert_eq!(shown.catch_up(&log), None, "nothing arrived");
        log.push("Writing… 🙂\n".into());
        let edit = shown.catch_up(&log).unwrap();
        assert_eq!(edit, Edit { trim: 0, append: "Writing… 🙂\n".into() });
        apply(&mut view, &edit);
        assert_eq!(String::from_utf16(&view).unwrap(), log.text());
    }

    #[test]
    fn the_oldest_output_is_trimmed_from_the_view_as_from_the_log() {
        let mut log = Log::default();
        let mut shown = Shown::default();
        let mut view = Vec::new();
        for n in 0..LOG_LIMIT + 7 {
            log.push(format!("line {n} — é🙂\n"));
            if n % 3 == 0 {
                apply(&mut view, &shown.catch_up(&log).unwrap());
                assert_eq!(String::from_utf16(&view).unwrap(), log.text());
            }
        }
        let edit = shown.catch_up(&log).unwrap();
        assert!(edit.trim > 0);
        apply(&mut view, &edit);
        assert_eq!(String::from_utf16(&view).unwrap(), log.text());
        assert!(log.text().starts_with("line 7 "));
    }

    #[test]
    fn a_view_far_behind_is_replaced_whole() {
        let mut log = Log::default();
        let mut shown = Shown::default();
        let mut view = Vec::new();
        log.push("first\n".into());
        apply(&mut view, &shown.catch_up(&log).unwrap());
        for n in 0..LOG_LIMIT * 2 {
            log.push(format!("{n}\n"));
        }
        let edit = shown.catch_up(&log).unwrap();
        assert_eq!(edit.trim, "first\n".len());
        apply(&mut view, &edit);
        assert_eq!(String::from_utf16(&view).unwrap(), log.text());
    }
}
