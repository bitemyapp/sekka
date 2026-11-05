//! `wrap` as an enum of the four (pending × autowrap) states, with the
//! cursor and the row links as context.

use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Normal,
    Pending,
    NormalNoWrap,
    PendingNoWrap,
}

impl State {
    fn settled(self) -> State {
        match self {
            State::Pending => State::Normal,
            State::PendingNoWrap => State::NormalNoWrap,
            s => s,
        }
    }
    fn pending(self) -> State {
        match self {
            State::Normal => State::Pending,
            State::NormalNoWrap => State::PendingNoWrap,
            s => s,
        }
    }
    fn is_pending(self) -> bool {
        matches!(self, State::Pending | State::PendingNoWrap)
    }
    fn with_autowrap(self, on: bool) -> State {
        match (self.is_pending(), on) {
            (false, true) => State::Normal,
            (true, true) => State::Pending,
            (false, false) => State::NormalNoWrap,
            (true, false) => State::PendingNoWrap,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub row: u16,
    pub col: u16,
    pub pending: bool,
}

/// The machine. Its fields are private: the state changes only through
/// `Machine::handle`; [`Wrap::typed`] reads it.
pub struct Wrap {
    state: State,
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    /// Bit `r`: row `r` is soft-wrapped into row `r + 1`.
    links: u128,
}

impl Wrap {
    /// The current state, read-only.
    pub fn typed(&self) -> &State {
        &self.state
    }
}

impl Wrap {
    fn mask(&self) -> u128 {
        if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 }
    }

    #[inline]
    fn linefeed(&mut self, out: &mut impl Sink<Effect>) {
        self.state = self.state.settled();
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
        match self.state {
            State::Pending => self.soft_wrap(out),
            State::PendingNoWrap => self.state = State::NormalNoWrap,
            State::Normal | State::NormalNoWrap => {}
        }
        if wide && self.col + 1 >= self.cols {
            if self.state == State::NormalNoWrap {
                return;
            }
            out.push(Effect::SpacerHead { row: self.row, col: self.col });
            self.soft_wrap(out);
        }
        out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col += if wide { 2 } else { 1 };
        if self.col >= self.cols {
            self.col = self.cols - 1;
            self.state = self.state.pending();
        }
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self {
            state: State::Normal,
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
                self.state = self.state.settled();
            }
            Event::LineFeed => self.linefeed(out),
            Event::Backspace => {
                self.col = self.col.saturating_sub(1);
                self.state = self.state.settled();
            }
            Event::Tab => {
                let last = self.cols - 1;
                let next = (self.col / 8 + 1) * 8;
                self.col = if next <= last { next } else { last };
            }
            Event::CursorTo { row, col } => {
                self.row = row.min(self.rows - 1);
                self.col = col.min(self.cols - 1);
                self.state = self.state.settled();
            }
            Event::ReverseIndex => {
                if self.row == 0 {
                    out.push(Effect::ScrollDown);
                    self.links = (self.links << 1) & self.mask();
                } else {
                    self.row -= 1;
                    self.state = self.state.settled();
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
                self.state = self.state.settled();
            }
            Event::SetAutowrap(on) => self.state = self.state.with_autowrap(on),
            Event::SaveCursor => {
                self.saved = Saved { row: self.row, col: self.col, pending: self.state.is_pending() };
            }
            Event::RestoreCursor => {
                let on = matches!(self.state, State::Normal | State::Pending);
                self.row = self.saved.row;
                self.col = self.saved.col;
                let base = if self.saved.pending { State::Pending } else { State::Normal };
                self.state = base.with_autowrap(on);
            }
            Event::FullReset => {
                *self = Self::new(Config { cols: self.cols, rows: self.rows });
            }
        }
    }

    fn state(&self) -> StateName {
        match self.state {
            State::Normal => StateName::Normal,
            State::Pending => StateName::PendingWrap,
            State::NormalNoWrap => StateName::NormalNoAutowrap,
            State::PendingNoWrap => StateName::PendingNoAutowrap,
        }
    }
}
