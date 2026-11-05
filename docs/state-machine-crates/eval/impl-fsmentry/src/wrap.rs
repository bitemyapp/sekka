//! `wrap` with fsmentry 0.4.0: the four (pending × autowrap) states as one
//! graph of unit states with named edges (`pend`, `settle`, `autowrap_off`,
//! `autowrap_on`); the cursor, the row links, the saved cursor and REP's
//! memory are the machine's own fields beside it.

use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

pub const IMPLEMENTED: bool = true;

pub mod mode {
    fsmentry::fsmentry! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Mode {
            // A print reaching the last column (or a restored pending cursor), and back.
            Normal -pend-> PendingWrap -settle-> Normal,
            NormalNoAutowrap -pend-> PendingNoAutowrap -settle-> NormalNoAutowrap,
            // DECAWM keeps the pending wrap.
            Normal -autowrap_off-> NormalNoAutowrap -autowrap_on-> Normal,
            PendingWrap -autowrap_off-> PendingNoAutowrap -autowrap_on-> PendingWrap,
        }
    }
}

pub use mode::{Mode, ModeEntry};

impl Mode {
    /// Clear a pending wrap (CR, LF, BS, CUP, RI, EL).
    fn settle(&mut self) {
        match self.entry() {
            ModeEntry::PendingWrap(p) => p.settle(),
            ModeEntry::PendingNoAutowrap(p) => p.settle(),
            ModeEntry::Normal(_) | ModeEntry::NormalNoAutowrap(_) => {}
        }
    }

    fn is_pending(&self) -> bool {
        matches!(self, Mode::PendingWrap | Mode::PendingNoAutowrap)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Saved {
    row: u16,
    col: u16,
    pending: bool,
}

/// The graph's state beside the cursor, the saved cursor, REP's memory and
/// the soft-wrap links (bit `r`: row `r` continues on `r + 1`).
pub struct Wrap {
    mode: Mode,
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    links: u128,
}

impl Wrap {
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// fsmentry's transition API on the live machine: only the current
    /// state's legal edges (and none of `handle`'s effects or cursor moves).
    pub fn entry(&mut self) -> ModeEntry<'_> {
        self.mode.entry()
    }

    #[inline]
    fn linefeed(&mut self, out: &mut impl Sink<Effect>) {
        if self.row == self.rows - 1 {
            out.push(Effect::ScrollUp);
            self.links >>= 1;
        } else {
            self.row += 1;
        }
    }

    #[cold]
    fn soft_wrap(&mut self, out: &mut impl Sink<Effect>) {
        out.push(Effect::SoftWrap { row: self.row });
        self.links |= 1 << self.row;
        self.linefeed(out);
        self.col = 0;
    }

    #[inline]
    fn print(&mut self, wide: bool, out: &mut impl Sink<Effect>) {
        self.previous = Some(wide);
        let autowrap = match self.mode.entry() {
            ModeEntry::Normal(_) => true,
            ModeEntry::NormalNoAutowrap(_) => false,
            ModeEntry::PendingWrap(p) => {
                p.settle();
                self.soft_wrap(out);
                true
            }
            // Without autowrap the print overwrites the last column.
            ModeEntry::PendingNoAutowrap(p) => {
                p.settle();
                false
            }
        };
        if wide && self.col + 1 >= self.cols {
            if !autowrap {
                return;
            }
            out.push(Effect::SpacerHead { row: self.row, col: self.col });
            self.soft_wrap(out);
        }
        out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col += if wide { 2 } else { 1 };
        if self.col >= self.cols {
            self.col = self.cols - 1;
            match self.mode.entry() {
                ModeEntry::Normal(n) => n.pend(),
                ModeEntry::NormalNoAutowrap(n) => n.pend(),
                ModeEntry::PendingWrap(_) | ModeEntry::PendingNoAutowrap(_) => {}
            }
        }
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self {
            mode: Mode::Normal,
            cols: config.cols,
            rows: config.rows,
            row: 0,
            col: 0,
            saved: Saved::default(),
            previous: None,
            links: 0,
        }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        match event {
            Event::Print { wide } => self.print(wide, out),
            Event::Repeat(n) => {
                if let Some(wide) = self.previous {
                    for _ in 0..n.max(1) {
                        self.print(wide, out);
                    }
                }
            }
            Event::CarriageReturn => {
                self.col = 0;
                self.mode.settle();
            }
            Event::LineFeed => {
                self.mode.settle();
                self.linefeed(out);
            }
            Event::Backspace => {
                self.col = self.col.saturating_sub(1);
                self.mode.settle();
            }
            // HT keeps the pending wrap.
            Event::Tab => self.col = ((self.col / 8 + 1) * 8).min(self.cols - 1),
            Event::CursorTo { row, col } => {
                self.row = row.min(self.rows - 1);
                self.col = col.min(self.cols - 1);
                self.mode.settle();
            }
            Event::ReverseIndex => {
                if self.row == 0 {
                    out.push(Effect::ScrollDown);
                    let mask = if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 };
                    self.links = (self.links << 1) & mask;
                } else {
                    self.row -= 1;
                    self.mode.settle();
                }
            }
            Event::EraseLine(m) => {
                if m > 2 {
                    return;
                }
                if m != 1 && self.links & (1 << self.row) != 0 {
                    self.links &= !(1 << self.row);
                    out.push(Effect::Unlink { row: self.row });
                }
                self.mode.settle();
            }
            Event::SetAutowrap(on) => match (self.mode.entry(), on) {
                (ModeEntry::Normal(m), false) => m.autowrap_off(),
                (ModeEntry::PendingWrap(m), false) => m.autowrap_off(),
                (ModeEntry::NormalNoAutowrap(m), true) => m.autowrap_on(),
                (ModeEntry::PendingNoAutowrap(m), true) => m.autowrap_on(),
                _ => {}
            },
            Event::SaveCursor => self.saved = Saved { row: self.row, col: self.col, pending: self.mode.is_pending() },
            Event::RestoreCursor => {
                self.row = self.saved.row;
                self.col = self.saved.col;
                match (self.mode.entry(), self.saved.pending) {
                    (ModeEntry::Normal(m), true) => m.pend(),
                    (ModeEntry::NormalNoAutowrap(m), true) => m.pend(),
                    (ModeEntry::PendingWrap(m), false) => m.settle(),
                    (ModeEntry::PendingNoAutowrap(m), false) => m.settle(),
                    _ => {}
                }
            }
            // RIS: a fresh machine (fsmentry has no reset; the state enum's
            // initial variant is just a value).
            Event::FullReset => *self = Self::new(Config { cols: self.cols, rows: self.rows }),
        }
    }

    fn state(&self) -> StateName {
        match self.mode {
            Mode::Normal => StateName::Normal,
            Mode::PendingWrap => StateName::PendingWrap,
            Mode::NormalNoAutowrap => StateName::NormalNoAutowrap,
            Mode::PendingNoAutowrap => StateName::PendingNoAutowrap,
        }
    }
}
