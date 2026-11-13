//! `wrap` with rust-fsm 0.8.0: the four (pending × autowrap) states are the
//! table (`fsm`); the cursor, the saved cursor, REP's memory and the row
//! links live beside it in [`Data`].
//!
//! What the DSL can't hold, and where it went:
//!
//! - whether a print leaves a wrap pending depends on the column it ends
//!   at, known only after the action ran: the print actions feed a
//!   completion input, `Reached`, back into the table (a second `consume`);
//! - guards on the data (`ReverseIndex` at the top, `RestoreCursor` of a
//!   pending cursor) are split inputs, chosen by [`Wrap::input`];
//! - `Repeat(n)` would change state between its prints, so it never reaches
//!   the table: `handle` expands it into `n` prints;
//! - the table's states are `spec::wrap::StateName` (the crate's custom
//!   state type), so `state()` reads the table directly;
//! - `SaveCursor` stores the state's pending bit: the output names it
//!   (`Save` / `SavePending`), as a Mealy output may depend on the state.

use rust_fsm::state_machine;
use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

pub const IMPLEMENTED: bool = true;

state_machine! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[state_machine(state(spec::wrap::StateName))]
    pub fsm(Normal)

    Normal => {
        Print => Normal [Print],
        Reached => PendingWrap,
        CarriageReturn => Normal [Cr],
        LineFeed => Normal [Lf],
        Backspace => Normal [Bs],
        Tab => Normal [Tab],
        CursorTo => Normal [Cup],
        ReverseIndexTop => Normal [ScrollDown],
        ReverseIndex => Normal [RowUp],
        EraseLine => Normal [Erase],
        AutowrapOff => NormalNoAutowrap,
        SaveCursor => Normal [Save],
        Restore => Normal [Restore],
        RestorePending => PendingWrap [Restore],
        FullReset => Normal [Reset],
    },
    PendingWrap => {
        Print => Normal [WrapPrint],
        CarriageReturn => Normal [Cr],
        LineFeed => Normal [Lf],
        Backspace => Normal [Bs],
        Tab => PendingWrap [Tab],
        CursorTo => Normal [Cup],
        ReverseIndexTop => PendingWrap [ScrollDown],
        ReverseIndex => Normal [RowUp],
        EraseLine => Normal [Erase],
        AutowrapOff => PendingNoAutowrap,
        SaveCursor => PendingWrap [SavePending],
        Restore => Normal [Restore],
        RestorePending => PendingWrap [Restore],
        FullReset => Normal [Reset],
    },
    NormalNoAutowrap => {
        Print => NormalNoAutowrap [PrintNoWrap],
        Reached => PendingNoAutowrap,
        CarriageReturn => NormalNoAutowrap [Cr],
        LineFeed => NormalNoAutowrap [Lf],
        Backspace => NormalNoAutowrap [Bs],
        Tab => NormalNoAutowrap [Tab],
        CursorTo => NormalNoAutowrap [Cup],
        ReverseIndexTop => NormalNoAutowrap [ScrollDown],
        ReverseIndex => NormalNoAutowrap [RowUp],
        EraseLine => NormalNoAutowrap [Erase],
        AutowrapOn => Normal,
        SaveCursor => NormalNoAutowrap [Save],
        Restore => NormalNoAutowrap [Restore],
        RestorePending => PendingNoAutowrap [Restore],
        FullReset => Normal [Reset],
    },
    PendingNoAutowrap => {
        Print => NormalNoAutowrap [PrintNoWrap],
        CarriageReturn => NormalNoAutowrap [Cr],
        LineFeed => NormalNoAutowrap [Lf],
        Backspace => NormalNoAutowrap [Bs],
        Tab => PendingNoAutowrap [Tab],
        CursorTo => NormalNoAutowrap [Cup],
        ReverseIndexTop => PendingNoAutowrap [ScrollDown],
        ReverseIndex => NormalNoAutowrap [RowUp],
        EraseLine => NormalNoAutowrap [Erase],
        AutowrapOn => PendingWrap,
        SaveCursor => PendingNoAutowrap [SavePending],
        Restore => NormalNoAutowrap [Restore],
        RestorePending => PendingNoAutowrap [Restore],
        FullReset => Normal [Reset],
    },
}

use fsm::{Input, Output};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub row: u16,
    pub col: u16,
    pub pending: bool,
}

/// The table and the data it can't hold (all of it lives in every state), in
/// one struct so the table's byte packs with the data.
#[derive(Clone, Debug)]
pub struct Data {
    fsm: fsm::StateMachine,
    pub cols: u16,
    pub rows: u16,
    pub row: u16,
    pub col: u16,
    pub saved: Saved,
    pub previous: Option<bool>,
    /// Bit `r`: row `r` is soft-wrapped into row `r + 1`.
    pub links: u128,
}

pub struct Wrap {
    data: Data,
}

impl Data {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            fsm: fsm::StateMachine::new(),
            cols,
            rows,
            row: 0,
            col: 0,
            saved: Saved::default(),
            previous: None,
            links: 0,
        }
    }

    fn mask(&self) -> u128 {
        if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 }
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

    /// Puts the scalar and advances; true when it reached the last column.
    #[inline]
    fn put(&mut self, wide: bool, out: &mut impl Sink<Effect>) -> bool {
        out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col += if wide { 2 } else { 1 };
        if self.col >= self.cols {
            self.col = self.cols - 1;
            return true;
        }
        false
    }
}

impl Wrap {
    /// The table's state.
    pub fn table(&self) -> &fsm::StateMachine {
        &self.data.fsm
    }

    /// The data beside the table.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// The guards: an event refined by what the data says about it.
    #[inline]
    fn input(&self, event: &Event) -> Option<Input> {
        Some(match *event {
            Event::Print { .. } => Input::Print,
            Event::Repeat(_) => return None,
            Event::CarriageReturn => Input::CarriageReturn,
            Event::LineFeed => Input::LineFeed,
            Event::Backspace => Input::Backspace,
            Event::Tab => Input::Tab,
            Event::CursorTo { .. } => Input::CursorTo,
            Event::ReverseIndex if self.data.row == 0 => Input::ReverseIndexTop,
            Event::ReverseIndex => Input::ReverseIndex,
            Event::EraseLine(m) if m > 2 => return None,
            Event::EraseLine(_) => Input::EraseLine,
            Event::SetAutowrap(true) => Input::AutowrapOn,
            Event::SetAutowrap(false) => Input::AutowrapOff,
            Event::SaveCursor => Input::SaveCursor,
            Event::RestoreCursor if self.data.saved.pending => Input::RestorePending,
            Event::RestoreCursor => Input::Restore,
            Event::FullReset => Input::FullReset,
        })
    }

    #[inline]
    fn step(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let Some(input) = self.input(&event) else { return };
        // `Err`: no row for this (state, input): the spec ignores the event.
        if let Ok(Some(action)) = self.data.fsm.consume(&input) {
            self.act(action, event, out);
        }
    }

    /// The actions the outputs name. Exhaustive on the output, so a new
    /// output is a compile error; arms that need a payload take it from
    /// their event (the table pairs every output with one kind of event).
    #[inline]
    fn act(&mut self, action: Output, event: Event, out: &mut impl Sink<Effect>) {
        let d = &mut self.data;
        let reached = match action {
            Output::Print => {
                let Event::Print { wide } = event else { unreachable!() };
                d.previous = Some(wide);
                if wide && d.col + 1 >= d.cols {
                    out.push(Effect::SpacerHead { row: d.row, col: d.col });
                    d.soft_wrap(out);
                }
                d.put(wide, out)
            }
            Output::WrapPrint => {
                let Event::Print { wide } = event else { unreachable!() };
                d.previous = Some(wide);
                d.soft_wrap(out);
                d.put(wide, out)
            }
            Output::PrintNoWrap => {
                let Event::Print { wide } = event else { unreachable!() };
                d.previous = Some(wide);
                // Without autowrap a wide scalar that doesn't fit is dropped.
                !(wide && d.col + 1 >= d.cols) && d.put(wide, out)
            }
            Output::Cr => {
                d.col = 0;
                false
            }
            Output::Lf => {
                d.linefeed(out);
                false
            }
            Output::Bs => {
                d.col = d.col.saturating_sub(1);
                false
            }
            Output::Tab => {
                let last = d.cols - 1;
                let next = (d.col / 8 + 1) * 8;
                d.col = if next <= last { next } else { last };
                false
            }
            Output::Cup => {
                let Event::CursorTo { row, col } = event else { unreachable!() };
                d.row = row.min(d.rows - 1);
                d.col = col.min(d.cols - 1);
                false
            }
            Output::ScrollDown => {
                out.push(Effect::ScrollDown);
                d.links = (d.links << 1) & d.mask();
                false
            }
            Output::RowUp => {
                d.row -= 1;
                false
            }
            Output::Erase => {
                let Event::EraseLine(m) = event else { unreachable!() };
                if m != 1 && d.links & (1 << d.row) != 0 {
                    d.links &= !(1 << d.row);
                    out.push(Effect::Unlink { row: d.row });
                }
                false
            }
            Output::Save | Output::SavePending => {
                d.saved = Saved { row: d.row, col: d.col, pending: action == Output::SavePending };
                false
            }
            Output::Restore => {
                d.row = d.saved.row;
                d.col = d.saved.col;
                false
            }
            Output::Reset => {
                // The table's `FullReset` row has already gone to `Normal`.
                (d.row, d.col, d.saved, d.previous, d.links) = (0, 0, Saved::default(), None, 0);
                false
            }
        };
        if reached {
            // Always `Ok`: every print row leads to a state with a `Reached` row.
            let r = d.fsm.consume(&Input::Reached);
            debug_assert!(r.is_ok());
        }
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self { data: Data::new(config.cols, config.rows) }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        match event {
            Event::Repeat(n) => {
                if let Some(wide) = self.data.previous {
                    for _ in 0..n.max(1) {
                        self.step(Event::Print { wide }, out);
                    }
                }
            }
            event => self.step(event, out),
        }
    }

    fn state(&self) -> StateName {
        *self.data.fsm.state()
    }
}
