//! `wrap` with smlang 0.8.0: the four (pending × autowrap) states, flat and
//! dataless; the cursor, saved cursor, REP memory and row links are the
//! context. A transition's target is fixed per row, so where a print may or
//! may not leave a wrap pending, a guard (`fills_*`) predicts the action's
//! outcome. `Repeat(n)` can't be one row (its end state depends on `n`
//! prints), so `handle` feeds it to the machine as `n` prints.

use smlang::statemachine;
use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

pub const IMPLEMENTED: bool = true;

/// The generated machine, kept private: its `new_with_state` and
/// `context_mut` would let callers forge a state.
mod fsm {
    use super::*;

    statemachine! {
        temporary_context: &mut impl Sink<Effect>,
        derive_states: [Clone, Copy, Debug],
        derive_events: [Clone, Copy, Debug],
        transitions: {
            *Normal + Print(bool) [fills_normal] / print_normal = PendingWrap,
            Normal + Print(bool) / print_normal = Normal,
            PendingWrap + Print(bool) [fills_pending] / print_pending = PendingWrap,
            PendingWrap + Print(bool) / print_pending = Normal,
            // Without autowrap a pending print overwrites the last column: the
            // same print as from `NormalNoAutowrap`.
            NormalNoAutowrap | PendingNoAutowrap + Print(bool) [fills_no_autowrap] / print_no_autowrap = PendingNoAutowrap,
            NormalNoAutowrap | PendingNoAutowrap + Print(bool) / print_no_autowrap = NormalNoAutowrap,
            Normal | PendingWrap + CarriageReturn / carriage_return = Normal,
            NormalNoAutowrap | PendingNoAutowrap + CarriageReturn / carriage_return = NormalNoAutowrap,
            Normal | PendingWrap + LineFeed / linefeed = Normal,
            NormalNoAutowrap | PendingNoAutowrap + LineFeed / linefeed = NormalNoAutowrap,
            Normal | PendingWrap + Backspace / backspace = Normal,
            NormalNoAutowrap | PendingNoAutowrap + Backspace / backspace = NormalNoAutowrap,
            _ + Tab / tab,
            Normal | PendingWrap + CursorTo((u16, u16)) / cursor_to = Normal,
            NormalNoAutowrap | PendingNoAutowrap + CursorTo((u16, u16)) / cursor_to = NormalNoAutowrap,
            _ + ReverseIndex [at_top] / scroll_down,
            Normal | PendingWrap + ReverseIndex / reverse_index = Normal,
            NormalNoAutowrap | PendingNoAutowrap + ReverseIndex / reverse_index = NormalNoAutowrap,
            Normal | PendingWrap + EraseLine(u8) [erases] / erase_line = Normal,
            NormalNoAutowrap | PendingNoAutowrap + EraseLine(u8) [erases] / erase_line = NormalNoAutowrap,
            Normal | NormalNoAutowrap + SetAutowrap(bool) [turned_on] = Normal,
            Normal | NormalNoAutowrap + SetAutowrap(bool) = NormalNoAutowrap,
            PendingWrap | PendingNoAutowrap + SetAutowrap(bool) [turned_on] = PendingWrap,
            PendingWrap | PendingNoAutowrap + SetAutowrap(bool) = PendingNoAutowrap,
            Normal | NormalNoAutowrap + SaveCursor / save_settled,
            PendingWrap | PendingNoAutowrap + SaveCursor / save_pending,
            Normal | PendingWrap + RestoreCursor [saved_pending] / restore = PendingWrap,
            Normal | PendingWrap + RestoreCursor / restore = Normal,
            NormalNoAutowrap | PendingNoAutowrap + RestoreCursor [saved_pending] / restore = PendingNoAutowrap,
            NormalNoAutowrap | PendingNoAutowrap + RestoreCursor / restore = NormalNoAutowrap,
            _ + FullReset / reset = Normal,
        }
    }
}

pub use fsm::{Error, Events, States};
use fsm::{StateMachine, StateMachineContext};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub row: u16,
    pub col: u16,
    pub pending: bool,
}

/// The context: every state's data.
pub struct Cursor {
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    /// Bit `r`: row `r` is soft-wrapped into row `r + 1`.
    links: u128,
}

impl Cursor {
    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self { cols: config.cols, rows: config.rows, row: 0, col: 0, saved: Saved::default(), previous: None, links: 0 }
    }

    pub fn position(&self) -> (u16, u16) {
        (self.row, self.col)
    }

    pub fn links(&self) -> u128 {
        self.links
    }

    fn mask(&self) -> u128 {
        if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 }
    }

    #[inline]
    fn line_feed(&mut self, out: &mut impl Sink<Effect>) {
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
        self.line_feed(out);
        self.col = 0;
    }

    /// Whether printing leaves a wrap pending (the guards' half of print).
    #[inline]
    fn fills(&self, wrap_first: bool, autowrap: bool, wide: bool) -> bool {
        let mut col = if wrap_first { 0 } else { self.col };
        if wide && col + 1 >= self.cols {
            if !autowrap {
                return false;
            }
            col = 0;
        }
        col + if wide { 2 } else { 1 } >= self.cols
    }

    /// The actions' half: the effects and the cursor; the row's target
    /// state says whether a wrap is pending afterwards.
    #[inline]
    fn print(&mut self, wrap_first: bool, autowrap: bool, wide: bool, out: &mut impl Sink<Effect>) {
        self.previous = Some(wide);
        if wrap_first {
            self.soft_wrap(out);
        }
        if wide && self.col + 1 >= self.cols {
            if !autowrap {
                return;
            }
            out.push(Effect::SpacerHead { row: self.row, col: self.col });
            self.soft_wrap(out);
        }
        out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col = (self.col + if wide { 2 } else { 1 }).min(self.cols - 1);
    }
}

type R = Result<(), ()>;

impl StateMachineContext for Cursor {
    fn fills_normal(&self, _: &mut impl Sink<Effect>, wide: &bool) -> Result<bool, ()> {
        Ok(self.fills(false, true, *wide))
    }
    fn fills_pending(&self, _: &mut impl Sink<Effect>, wide: &bool) -> Result<bool, ()> {
        Ok(self.fills(true, true, *wide))
    }
    fn fills_no_autowrap(&self, _: &mut impl Sink<Effect>, wide: &bool) -> Result<bool, ()> {
        Ok(self.fills(false, false, *wide))
    }
    fn at_top(&self, _: &mut impl Sink<Effect>) -> Result<bool, ()> {
        Ok(self.row == 0)
    }
    fn erases(&self, _: &mut impl Sink<Effect>, m: &u8) -> Result<bool, ()> {
        Ok(*m <= 2)
    }
    fn turned_on(&self, _: &mut impl Sink<Effect>, on: &bool) -> Result<bool, ()> {
        Ok(*on)
    }
    fn saved_pending(&self, _: &mut impl Sink<Effect>) -> Result<bool, ()> {
        Ok(self.saved.pending)
    }

    #[inline]
    fn print_normal(&mut self, out: &mut impl Sink<Effect>, wide: bool) -> R {
        self.print(false, true, wide, out);
        Ok(())
    }
    fn print_pending(&mut self, out: &mut impl Sink<Effect>, wide: bool) -> R {
        self.print(true, true, wide, out);
        Ok(())
    }
    fn print_no_autowrap(&mut self, out: &mut impl Sink<Effect>, wide: bool) -> R {
        self.print(false, false, wide, out);
        Ok(())
    }
    fn carriage_return(&mut self, _: &mut impl Sink<Effect>) -> R {
        self.col = 0;
        Ok(())
    }
    fn linefeed(&mut self, out: &mut impl Sink<Effect>) -> R {
        self.line_feed(out);
        Ok(())
    }
    fn backspace(&mut self, _: &mut impl Sink<Effect>) -> R {
        self.col = self.col.saturating_sub(1);
        Ok(())
    }
    fn tab(&mut self, _: &mut impl Sink<Effect>) -> R {
        let last = self.cols - 1;
        let next = (self.col / 8 + 1) * 8;
        self.col = if next <= last { next } else { last };
        Ok(())
    }
    fn cursor_to(&mut self, _: &mut impl Sink<Effect>, (row, col): (u16, u16)) -> R {
        self.row = row.min(self.rows - 1);
        self.col = col.min(self.cols - 1);
        Ok(())
    }
    fn scroll_down(&mut self, out: &mut impl Sink<Effect>) -> R {
        out.push(Effect::ScrollDown);
        self.links = (self.links << 1) & self.mask();
        Ok(())
    }
    fn reverse_index(&mut self, _: &mut impl Sink<Effect>) -> R {
        self.row -= 1;
        Ok(())
    }
    fn erase_line(&mut self, out: &mut impl Sink<Effect>, m: u8) -> R {
        if m != 1 && self.links & (1 << self.row) != 0 {
            self.links &= !(1 << self.row);
            out.push(Effect::Unlink { row: self.row });
        }
        Ok(())
    }
    fn save_settled(&mut self, _: &mut impl Sink<Effect>) -> R {
        self.saved = Saved { row: self.row, col: self.col, pending: false };
        Ok(())
    }
    fn save_pending(&mut self, _: &mut impl Sink<Effect>) -> R {
        self.saved = Saved { row: self.row, col: self.col, pending: true };
        Ok(())
    }
    fn restore(&mut self, _: &mut impl Sink<Effect>) -> R {
        self.row = self.saved.row;
        self.col = self.saved.col;
        Ok(())
    }
    fn reset(&mut self, _: &mut impl Sink<Effect>) -> R {
        *self = Cursor::new(Config { cols: self.cols, rows: self.rows });
        Ok(())
    }
}

pub struct Wrap {
    sm: StateMachine<Cursor>,
}

impl Wrap {
    /// The generated `process_event`: the typed transition API.
    pub fn process_event(&mut self, out: &mut impl Sink<Effect>, event: Events) -> Result<&States, Error> {
        self.sm.process_event(out, event)
    }

    /// The generated state.
    pub fn state_enum(&self) -> &States {
        self.sm.state()
    }

    /// The context, read-only.
    pub fn context(&self) -> &Cursor {
        self.sm.context()
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        Self { sm: StateMachine::new(Cursor::new(config)) }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let event = match event {
            Event::Print { wide } => Events::Print(wide),
            Event::Repeat(n) => {
                if let Some(wide) = self.sm.context().previous {
                    for _ in 0..n.max(1) {
                        let _ = self.sm.process_event(out, Events::Print(wide));
                    }
                }
                return;
            }
            Event::CarriageReturn => Events::CarriageReturn,
            Event::LineFeed => Events::LineFeed,
            Event::Backspace => Events::Backspace,
            Event::Tab => Events::Tab,
            Event::CursorTo { row, col } => Events::CursorTo((row, col)),
            Event::ReverseIndex => Events::ReverseIndex,
            Event::EraseLine(m) => Events::EraseLine(m),
            Event::SetAutowrap(on) => Events::SetAutowrap(on),
            Event::SaveCursor => Events::SaveCursor,
            Event::RestoreCursor => Events::RestoreCursor,
            Event::FullReset => Events::FullReset,
        };
        // A failed guard (`EraseLine` above 2) is `Err`: ignored.
        let _ = self.sm.process_event(out, event);
    }

    fn state(&self) -> StateName {
        match self.sm.state() {
            States::Normal => StateName::Normal,
            States::PendingWrap => StateName::PendingWrap,
            States::NormalNoAutowrap => StateName::NormalNoAutowrap,
            States::PendingNoAutowrap => StateName::PendingNoAutowrap,
        }
    }
}
