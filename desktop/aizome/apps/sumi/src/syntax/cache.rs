//! Each line's starting state, kept between edits, so that an edit
//! re-tokenizes only the lines it touched and those after it whose
//! starting state it changed: once a line after the edit starts as it did
//! before, the rest of the file does too, up to the next edit.
//!
//! States are worked out lazily, as far as the editor shows, within a
//! time budget: past it, lines are styled from a guess (as if nothing were
//! open), and the program is told to call again so the work goes on
//! between frames.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use ropey::Rope;

use super::{Language, LineState, Out, Token, tokenize};

/// Lines a millisecond of working states out is taken to go through
/// until it's measured (a release build does three or four thousand of
/// Rust).
const GUESSED_RATE: f64 = 1000.0;

/// The text of line `i`, without its line break.
pub fn line_text(rope: &Rope, i: usize) -> Cow<'_, str> {
    let slice = rope.line(i);
    let mut s: Cow<str> = match slice.as_str() {
        Some(s) => Cow::Borrowed(s),
        None => Cow::Owned(slice.to_string()),
    };
    let trim = s.len() - s.trim_end_matches(['\n', '\r']).len();
    if trim > 0 {
        match &mut s {
            Cow::Borrowed(b) => *b = &b[..b.len() - trim],
            Cow::Owned(o) => o.truncate(o.len() - trim),
        }
    }
    s
}

/// A document's line states.
pub struct Highlight {
    lang: Language,
    /// `states[i]` is the state line `i` starts in.
    states: Vec<LineState>,
    /// `states[..=valid]` are right for the text as it is.
    valid: usize,
    /// Lines changed since (first, last, in today's numbering), past
    /// `valid`, in order.
    pending: Vec<(usize, usize)>,
    /// Guesses past what's worked out: `(first line, states)`.
    guess: Option<(usize, Vec<LineState>)>,
    /// How long one call may spend working states out.
    pub budget: Duration,
    /// A line was styled from a guess: there's work left.
    pub incomplete: bool,
    /// The program has been asked to call again.
    pub scheduled: bool,
    /// This pass is one the program was asked for: it spends the budget
    /// working states out even towards a line it can't reach.
    pub pump: bool,
    /// Lines a millisecond working states out goes through, as last
    /// measured.
    rate: f64,
    /// Lines tokenized to work states out (for tests and measuring).
    pub worked: u64,
    /// Asks the program to call again (when `incomplete`).
    pub notify: Option<Box<dyn Fn() + Send>>,
}

impl Highlight {
    pub fn new(lang: Language) -> Highlight {
        Highlight {
            lang,
            states: vec![LineState::default()],
            valid: 0,
            pending: Vec::new(),
            guess: None,
            budget: Duration::from_millis(3),
            incomplete: false,
            scheduled: false,
            pump: false,
            rate: GUESSED_RATE,
            worked: 0,
            notify: None,
        }
    }

    pub fn language(&self) -> Language {
        self.lang
    }

    /// Start over (the language changed, or the whole text).
    pub fn reset(&mut self, lang: Language) {
        let (budget, rate) = (self.budget, self.rate);
        let notify = self.notify.take();
        *self = Highlight::new(lang);
        (self.budget, self.rate) = (budget, rate);
        self.notify = notify;
    }

    /// How far states are known to be right.
    pub fn valid(&self) -> usize {
        self.valid
    }

    /// Lines `first..=old_last` were replaced by `first..=new_last`.
    pub fn edited(&mut self, first: usize, old_last: usize, new_last: usize) {
        self.guess = None;
        let len = self.states.len();
        if first >= len {
            return;
        }
        let old_last = old_last.max(first);
        let new_last = new_last.max(first);
        if old_last + 1 >= len {
            self.states.truncate(first + 1);
        } else {
            self.states.splice(first + 1..old_last + 1, std::iter::repeat_n(LineState::default(), new_last - first));
        }
        let delta = new_last as isize - old_last as isize;
        let shift = |l: usize| (l as isize + delta).max(0) as usize;
        let (mut a, mut b) = (first, new_last);
        let mut kept = Vec::with_capacity(self.pending.len() + 1);
        for &(x, y) in &self.pending {
            if y < first {
                kept.push((x, y));
            } else if x > old_last {
                kept.push((shift(x), shift(y)));
            } else {
                a = a.min(x);
                b = b.max(if y > old_last { shift(y) } else { new_last });
            }
        }
        kept.push((a, b));
        kept.sort_unstable();
        let len = self.states.len();
        kept.retain(|&(x, _)| x < len);
        self.pending = kept;
        self.valid = self.valid.min(first);
    }

    /// Work states out up to line `target` (of `rope`), until `deadline`:
    /// whether it got there.
    pub fn advance(&mut self, target: usize, rope: &Rope, deadline: Option<Instant>) -> bool {
        let total = rope.len_lines();
        let target = target.min(total.saturating_sub(1));
        let started = deadline.map(|_| Instant::now());
        let mut count = 0u32;
        while self.valid < target {
            let j = self.valid;
            // Regions whose last line is behind are done with.
            while self.pending.first().is_some_and(|&(_, last)| last < j) {
                self.pending.remove(0);
            }
            let text = line_text(rope, j);
            let s = tokenize(self.lang, self.states[j], &text, j, &mut Out(None));
            self.worked += 1;
            let next = j + 1;
            // Inside a change the next line's old state means nothing (it
            // may be a line that wasn't there); from its last line on, it's
            // the state the line after the change had.
            let changed_here = self.pending.first().is_some_and(|&(first, last)| first <= j && j < last);
            if next < self.states.len() {
                if !changed_here && self.states[next] == s {
                    // As before: right up to the next change.
                    let to = self
                        .pending
                        .iter()
                        .find(|&&(first, _)| first > j)
                        .map_or(self.states.len() - 1, |&(first, _)| first);
                    self.valid = to.max(next).min(self.states.len() - 1);
                    continue;
                }
                self.states[next] = s;
            } else {
                self.states.push(s);
            }
            self.valid = next;
            count += 1;
            if count.is_multiple_of(128)
                && let Some(d) = deadline
                && Instant::now() >= d
            {
                self.measured(count, started);
                return self.valid >= target;
            }
        }
        self.measured(count, started);
        if self.guess.as_ref().is_some_and(|(g, _)| *g <= self.valid) {
            self.guess = None;
        }
        true
    }

    /// How fast working states out went, from a run of `count` lines
    /// started at `started`: kept when the run was long enough to say.
    fn measured(&mut self, count: u32, started: Option<Instant>) {
        if let Some(t) = started
            && count >= 1024
        {
            let ms = t.elapsed().as_secs_f64() * 1e3;
            if ms > 0.0 {
                self.rate = f64::from(count) / ms;
            }
        }
    }

    /// Whether the budget could work states out as far as `line`: near
    /// enough at the rate measured, or behind an edit (where the states
    /// from before it soon match again, and the rest stand).
    fn within_reach(&self, line: usize) -> bool {
        line < self.states.len()
            || (line.saturating_sub(self.valid) as f64) <= self.rate * self.budget.as_secs_f64() * 1e3
    }

    /// The state line `line` starts in: worked out within the budget, else
    /// guessed.
    pub fn state_at(&mut self, line: usize, rope: &Rope) -> LineState {
        if line <= self.valid {
            return self.states[line];
        }
        // One budget a pass: once it ran out, the pass guesses. A pass that
        // couldn't get there within it (a jump far ahead) guesses at once,
        // leaving the work to the passes the program is asked for after,
        // between frames, rather than spending the budget in vain.
        if !self.incomplete && (self.pump || self.within_reach(line)) {
            self.pump = false;
            let deadline = Instant::now() + self.budget;
            if self.advance(line, rope, Some(deadline)) && line <= self.valid {
                return self.states[line];
            }
        }
        self.incomplete = true;
        // Guess: as if nothing were open where the guesses start, then on
        // from there.
        match &mut self.guess {
            Some((g, v)) if *g <= line && line <= *g + v.len() + 64 => {
                while *g + v.len() <= line {
                    let k = *g + v.len() - 1;
                    let text = line_text(rope, k);
                    let s = tokenize(self.lang, v[v.len() - 1], &text, k, &mut Out(None));
                    v.push(s);
                }
                v[line - *g]
            }
            _ => {
                self.guess = Some((line, vec![LineState::default()]));
                LineState::default()
            }
        }
    }

    /// Line `line`'s tokens (its text `text`, of `rope`).
    pub fn tokens(&mut self, line: usize, text: &str, rope: &Rope) -> Vec<Token> {
        if self.lang == Language::Plain {
            return Vec::new();
        }
        let state = self.state_at(line, rope);
        let mut v = Vec::new();
        tokenize(self.lang, state, text, line, &mut Out(Some(&mut v)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{Block, Kind, Mode};

    fn rope(s: &str) -> Rope {
        Rope::from_str(s)
    }

    /// The states from scratch, to compare with.
    fn fresh(lang: Language, r: &Rope) -> Vec<LineState> {
        let mut h = Highlight::new(lang);
        h.advance(r.len_lines(), r, None);
        (0..r.len_lines()).map(|i| h.state_at(i, r)).collect()
    }

    fn check(h: &mut Highlight, r: &Rope) {
        let want = fresh(h.language(), r);
        for (i, w) in want.iter().enumerate() {
            assert_eq!(h.state_at(i, r), *w, "line {i}");
        }
    }

    #[test]
    fn an_edit_works_out_only_what_it_changed() {
        let text: String = (0..1000).map(|i| format!("let x{i} = {i}; // line\n")).collect();
        let mut r = rope(&text);
        let mut h = Highlight::new(Language::Rust);
        h.budget = Duration::from_secs(60);
        h.advance(999, &r, None);
        let before = h.worked;
        // Type into line 10: one line, then it converges at line 11.
        let at = r.line_to_char(10) + 4;
        r.insert(at, "abc");
        h.edited(10, 10, 10);
        h.advance(999, &r, None);
        assert!(h.worked - before <= 2, "worked {} lines", h.worked - before);
        check(&mut h, &r);
        // Open a block comment on line 20: everything after changes.
        let at = r.line_to_char(20);
        r.insert(at, "/* ");
        h.edited(20, 20, 20);
        assert_eq!(h.state_at(500, &r).mode, Mode::Comment { depth: 1 });
        check(&mut h, &r);
        // Close it on line 30 with a new line: the rest comes back.
        let at = r.line_to_char(30);
        r.insert(at, "*/\n");
        h.edited(30, 30, 31);
        check(&mut h, &r);
        // Delete lines 40 to 49.
        let (a, b) = (r.line_to_char(40), r.line_to_char(50));
        r.remove(a..b);
        h.edited(40, 50, 40);
        check(&mut h, &r);
    }

    #[test]
    fn edits_far_apart_before_any_work() {
        let text: String = (0..300).map(|i| format!("line {i}\n")).collect();
        let mut r = rope(&text);
        let mut h = Highlight::new(Language::Rust);
        h.budget = Duration::from_secs(60);
        h.advance(299, &r, None);
        // Two edits, the later first, with no work between them.
        let at = r.line_to_char(200);
        r.insert(at, "/* ");
        h.edited(200, 200, 200);
        let at = r.line_to_char(10);
        r.insert(at, "a\nb\n");
        h.edited(10, 10, 12);
        assert_eq!(h.state_at(205, &r).mode, Mode::Comment { depth: 1 }, "the later edit isn't skipped");
        check(&mut h, &r);
    }

    #[test]
    fn a_fence_carries_its_language() {
        let r = rope("# Notes\n```rust\nfn a() {}\n```\ntext\n");
        let mut h = Highlight::new(Language::Markdown);
        h.budget = Duration::from_secs(60);
        let t = h.tokens(2, "fn a() {}", &r);
        assert_eq!(t[0].kind, Kind::Keyword);
        assert!(matches!(h.state_at(2, &r).block, Block::Fence { lang: Language::Rust, .. }));
        assert_eq!(h.state_at(4, &r), LineState::default());
    }

    #[test]
    fn out_of_budget_it_guesses_and_goes_on_later() {
        let text: String = (0..200_000).map(|i| format!("x{i}\n")).collect();
        let r = rope(&text);
        let mut h = Highlight::new(Language::Rust);
        h.budget = Duration::ZERO;
        let _ = h.state_at(150_000, &r);
        assert!(h.incomplete);
        assert!(h.valid() < 150_000);
        h.budget = Duration::from_secs(60);
        h.incomplete = false;
        let _ = h.state_at(150_000, &r);
        assert!(!h.incomplete);
        assert!(h.valid() >= 150_000);
    }

    #[test]
    fn a_jump_out_of_reach_guesses_at_once_and_the_passes_after_work() {
        let text: String = (0..200_000).map(|i| format!("x{i}\n")).collect();
        let r = rope(&text);
        let mut h = Highlight::new(Language::Rust);
        // A budget that reaches about a thousand lines at the guessed rate.
        h.budget = Duration::from_millis(1);
        let _ = h.state_at(150_000, &r);
        assert!(h.incomplete);
        assert_eq!(h.worked, 0, "nothing is worked out in vain");
        // A pass the program was asked for spends its budget.
        h.incomplete = false;
        h.pump = true;
        let _ = h.state_at(150_000, &r);
        assert!(h.worked > 0);
        assert!(!h.pump);
        // Near enough, and behind an edit: worked out at once (in a budget
        // that reaches 20,000 lines at the guessed rate).
        let mut h = Highlight::new(Language::Rust);
        h.budget = Duration::from_millis(20);
        let _ = h.state_at(400, &r);
        assert!(!h.incomplete && h.valid() >= 400);
        h.budget = Duration::from_secs(60);
        let _ = h.state_at(150_000, &r);
        h.budget = Duration::from_millis(20);
        h.edited(10, 10, 10);
        let _ = h.state_at(149_000, &r);
        assert!(!h.incomplete && h.valid() >= 149_000);
    }
}
