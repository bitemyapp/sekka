//! `wrap` with statig 0.4.1: the four leaves (pending × autowrap) under the
//! superstates `autowrap` and `no_autowrap`, which handle the cursor
//! controls; the leaves handle printing. The cursor, the saved cursor, REP's
//! memory and the row links are shared storage (`&mut self`): every state
//! has them. `Repeat(n)` is unrolled into `n` prints in `handle`, because a
//! statig handler returns one outcome and the state can change between the
//! prints.

use spec::Sink;
use spec::wrap::{Config, Effect, Event, MAX_ROWS, Machine, StateName};
use statig::prelude::*;

use crate::{Nowhere, Out};

pub const IMPLEMENTED: bool = true;

/// What DECSC saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Saved {
    row: u16,
    col: u16,
    pending: bool,
}

/// The shared storage: the screen's size and cursor, and the row links.
pub struct Screen {
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    /// Bit `r`: row `r` is soft-wrapped into row `r + 1`.
    links: u128,
}

impl Screen {
    fn new(cols: u16, rows: u16) -> Self {
        assert!(cols >= 2 && rows >= 1 && rows <= MAX_ROWS);
        Self { cols, rows, row: 0, col: 0, saved: Saved::default(), previous: None, links: 0 }
    }

    fn linefeed(&mut self, out: &mut Out<'_, Effect>) {
        if self.row == self.rows - 1 {
            out.push(Effect::ScrollUp);
            self.links >>= 1;
        } else {
            self.row += 1;
        }
    }

    #[cold]
    fn soft_wrap(&mut self, out: &mut Out<'_, Effect>) {
        out.push(Effect::SoftWrap { row: self.row });
        self.links |= 1 << self.row;
        self.linefeed(out);
        self.col = 0;
    }

    /// Prints a scalar with no wrap pending (steps 1, 3 and 4 of print):
    /// `true` when it left a wrap pending.
    #[inline]
    fn print(&mut self, wide: bool, autowrap: bool, out: &mut Out<'_, Effect>) -> bool {
        self.previous = Some(wide);
        if wide && self.col + 1 >= self.cols {
            if !autowrap {
                return false;
            }
            out.push(Effect::SpacerHead { row: self.row, col: self.col });
            self.soft_wrap(out);
        }
        out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col += if wide { 2 } else { 1 };
        if self.col >= self.cols {
            self.col = self.cols - 1;
            return true;
        }
        false
    }

    /// The cursor controls, the same in both modes: `Some(pending)` when the
    /// control sets the pending wrap, `None` when it keeps it.
    fn control(&mut self, event: &Event, out: &mut Out<'_, Effect>) -> Option<bool> {
        match *event {
            Event::CarriageReturn => self.col = 0,
            Event::LineFeed => self.linefeed(out),
            Event::Backspace => self.col = self.col.saturating_sub(1),
            Event::Tab => {
                let next = (self.col / 8 + 1) * 8;
                self.col = next.min(self.cols - 1);
                return None;
            }
            Event::CursorTo { row, col } => {
                self.row = row.min(self.rows - 1);
                self.col = col.min(self.cols - 1);
            }
            Event::ReverseIndex if self.row == 0 => {
                out.push(Effect::ScrollDown);
                let mask = if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 };
                self.links = (self.links << 1) & mask;
                return None;
            }
            Event::ReverseIndex => self.row -= 1,
            Event::EraseLine(m) if m <= 2 => {
                if m != 1 && self.links & (1 << self.row) != 0 {
                    self.links &= !(1 << self.row);
                    out.push(Effect::Unlink { row: self.row });
                }
            }
            Event::RestoreCursor => {
                (self.row, self.col) = (self.saved.row, self.saved.col);
                return Some(self.saved.pending);
            }
            _ => return None,
        }
        Some(false)
    }

    fn save(&mut self, pending: bool) {
        self.saved = Saved { row: self.row, col: self.col, pending };
    }
}

#[state_machine(initial = "State::normal()", state(derive(Debug, Clone, Copy, PartialEq, Eq)))]
impl Screen {
    #[superstate]
    fn autowrap(&mut self, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        if let Event::FullReset = event {
            *self = Screen::new(self.cols, self.rows);
            return Transition(State::normal());
        }
        match self.control(event, context) {
            Some(false) => Transition(State::normal()),
            Some(true) => Transition(State::pending_wrap()),
            None => Handled,
        }
    }

    #[state(superstate = "autowrap")]
    fn normal(&mut self, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Print { wide } => {
                if self.print(wide, true, context) {
                    Transition(State::pending_wrap())
                } else {
                    Handled
                }
            }
            Event::SetAutowrap(false) => Transition(State::normal_no_autowrap()),
            Event::SaveCursor => {
                self.save(false);
                Handled
            }
            _ => Super,
        }
    }

    /// The last column was printed: the next print wraps first.
    #[state(superstate = "autowrap")]
    fn pending_wrap(&mut self, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Print { wide } => {
                self.soft_wrap(context);
                if self.print(wide, true, context) { Handled } else { Transition(State::normal()) }
            }
            Event::SetAutowrap(false) => Transition(State::pending_no_autowrap()),
            Event::SaveCursor => {
                self.save(true);
                Handled
            }
            _ => Super,
        }
    }

    #[superstate]
    fn no_autowrap(&mut self, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        if let Event::FullReset = event {
            *self = Screen::new(self.cols, self.rows);
            return Transition(State::normal());
        }
        match self.control(event, context) {
            Some(false) => Transition(State::normal_no_autowrap()),
            Some(true) => Transition(State::pending_no_autowrap()),
            None => Handled,
        }
    }

    #[state(superstate = "no_autowrap")]
    fn normal_no_autowrap(&mut self, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Print { wide } => {
                if self.print(wide, false, context) {
                    Transition(State::pending_no_autowrap())
                } else {
                    Handled
                }
            }
            Event::SetAutowrap(true) => Transition(State::normal()),
            Event::SaveCursor => {
                self.save(false);
                Handled
            }
            _ => Super,
        }
    }

    /// The last column was printed: the next print overwrites it.
    #[state(superstate = "no_autowrap")]
    fn pending_no_autowrap(&mut self, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Print { wide } => {
                if self.print(wide, false, context) {
                    Handled
                } else {
                    Transition(State::normal_no_autowrap())
                }
            }
            Event::SetAutowrap(true) => Transition(State::pending_wrap()),
            Event::SaveCursor => {
                self.save(true);
                Handled
            }
            _ => Super,
        }
    }
}

pub struct Wrap {
    machine: statig::blocking::InitializedStateMachine<Screen>,
}

impl Wrap {
    pub fn state_enum(&self) -> &State {
        self.machine.state()
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        let machine = Screen::new(config.cols, config.rows).uninitialized_state_machine();
        Self { machine: machine.init_with_context(&mut Out(&mut Nowhere)) }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let context = &mut Out(out);
        match event {
            Event::Repeat(n) => {
                if let Some(wide) = self.machine.previous {
                    for _ in 0..n.max(1) {
                        self.machine.handle_with_context(&Event::Print { wide }, context);
                    }
                }
            }
            event => self.machine.handle_with_context(&event, context),
        }
    }

    fn state(&self) -> StateName {
        match self.machine.state() {
            State::Normal {} => StateName::Normal,
            State::PendingWrap {} => StateName::PendingWrap,
            State::NormalNoAutowrap {} => StateName::NormalNoAutowrap,
            State::PendingNoAutowrap {} => StateName::PendingNoAutowrap,
        }
    }
}
