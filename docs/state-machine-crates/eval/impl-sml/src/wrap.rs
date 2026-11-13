//! `wrap` with stateforward-sml 2.0.0: a flat table of the four
//! (pending × autowrap) states; the cursor, the saved cursor, REP's memory
//! and the row links are the context. Guards run before actions and pick
//! the row (so the target), so the print guards compute where a print
//! leaves the cursor before the action prints. Effects go through the
//! per-call temporary context, a `dyn Sink`.

use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

pub const IMPLEMENTED: bool = true;

pub type Out<'a> = &'a mut dyn Sink<Effect>;

#[derive(Clone, Copy)]
pub struct Print {
    pub wide: bool,
}
pub struct Repeat(pub u16);
pub struct CarriageReturn;
pub struct LineFeed;
pub struct Backspace;
pub struct Tab;
pub struct CursorTo {
    pub row: u16,
    pub col: u16,
}
pub struct ReverseIndex;
pub struct EraseLine(pub u8);
pub struct SetAutowrap(pub bool);
pub struct SaveCursor;
pub struct RestoreCursor;
pub struct FullReset;

sml::sml! {
    Wrap[temporary_context: &mut dyn Sink<Effect>] {
        *Normal + event<Print> [fills] / print = PendingWrap,
         Normal + event<Print> / print,
         PendingWrap + event<Print> [fills_after_wrap] / wrap_print = PendingWrap,
         PendingWrap + event<Print> / wrap_print = Normal,
         NormalNoAutowrap + event<Print> [fills_no_autowrap] / print_no_autowrap = PendingNoAutowrap,
         NormalNoAutowrap + event<Print> / print_no_autowrap,
         PendingNoAutowrap + event<Print> [fills_again] / overwrite = PendingNoAutowrap,
         PendingNoAutowrap + event<Print> / overwrite = NormalNoAutowrap,

         Normal + event<Repeat> [repeat_fills] / repeat = PendingWrap,
         Normal + event<Repeat> / repeat,
         PendingWrap + event<Repeat> [repeat_fills_after_wrap] / repeat_wrapping = PendingWrap,
         PendingWrap + event<Repeat> [repeats] / repeat_wrapping = Normal,
         NormalNoAutowrap + event<Repeat> [repeat_fills_no_autowrap] / repeat_no_autowrap = PendingNoAutowrap,
         NormalNoAutowrap + event<Repeat> / repeat_no_autowrap,
         PendingNoAutowrap + event<Repeat> [repeat_fills_again] / repeat_overwriting = PendingNoAutowrap,
         PendingNoAutowrap + event<Repeat> [repeats] / repeat_overwriting = NormalNoAutowrap,

         PendingWrap + event<CarriageReturn> / carriage_return = Normal,
         PendingNoAutowrap + event<CarriageReturn> / carriage_return = NormalNoAutowrap,
         _ + event<CarriageReturn> / carriage_return,
         PendingWrap + event<LineFeed> / line_feed = Normal,
         PendingNoAutowrap + event<LineFeed> / line_feed = NormalNoAutowrap,
         _ + event<LineFeed> / line_feed,
         PendingWrap + event<Backspace> / backspace = Normal,
         PendingNoAutowrap + event<Backspace> / backspace = NormalNoAutowrap,
         _ + event<Backspace> / backspace,
         PendingWrap + event<CursorTo> / cursor_to = Normal,
         PendingNoAutowrap + event<CursorTo> / cursor_to = NormalNoAutowrap,
         _ + event<CursorTo> / cursor_to,
         PendingWrap + event<EraseLine> [erases] / erase_line = Normal,
         PendingNoAutowrap + event<EraseLine> [erases] / erase_line = NormalNoAutowrap,
         _ + event<EraseLine> [erases] / erase_line,
         _ + event<ReverseIndex> [at_top] / scroll_down,
         PendingWrap + event<ReverseIndex> / up = Normal,
         PendingNoAutowrap + event<ReverseIndex> / up = NormalNoAutowrap,
         Normal | NormalNoAutowrap + event<ReverseIndex> / up,
         _ + event<Tab> / tab,

         Normal + event<SetAutowrap> [turns_off] = NormalNoAutowrap,
         PendingWrap + event<SetAutowrap> [turns_off] = PendingNoAutowrap,
         NormalNoAutowrap + event<SetAutowrap> [turns_on] = Normal,
         PendingNoAutowrap + event<SetAutowrap> [turns_on] = PendingWrap,
         PendingWrap | PendingNoAutowrap + event<SaveCursor> / save_pending,
         _ + event<SaveCursor> / save,
         Normal | PendingWrap + event<RestoreCursor> [saved_pending] / restore = PendingWrap,
         Normal | PendingWrap + event<RestoreCursor> / restore = Normal,
         NormalNoAutowrap | PendingNoAutowrap + event<RestoreCursor> [saved_pending] / restore = PendingNoAutowrap,
         NormalNoAutowrap | PendingNoAutowrap + event<RestoreCursor> / restore = NormalNoAutowrap,
         _ + event<FullReset> / reset = Normal,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    row: u16,
    col: u16,
    pending: bool,
}

/// The context: the screen size, the cursor and the row links.
#[derive(Clone, Copy)]
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
        Self { cols, rows, row: 0, col: 0, saved: Saved::default(), previous: None, links: 0 }
    }

    fn linefeed(&mut self, out: Out<'_>) {
        if self.row == self.rows - 1 {
            out.push(Effect::ScrollUp);
            self.links >>= 1;
        } else {
            self.row += 1;
        }
    }

    fn soft_wrap(&mut self, out: Out<'_>) {
        out.push(Effect::SoftWrap { row: self.row });
        self.links |= 1 << self.row;
        self.linefeed(out);
        self.col = 0;
    }

    /// print(wide) from the state's (pending, autowrap).
    #[inline]
    fn print(&mut self, out: Out<'_>, pending: bool, autowrap: bool, wide: bool) {
        self.previous = Some(wide);
        if pending && autowrap {
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
        self.col = (self.col + 1 + u16::from(wide)).min(self.cols - 1);
    }

    /// The column and pending flag a print leaves, without printing.
    #[inline]
    fn after(&self, col: u16, pending: bool, autowrap: bool, wide: bool) -> (u16, bool) {
        let mut col = if pending && autowrap { 0 } else { col };
        if wide && col + 1 >= self.cols {
            if !autowrap {
                return (col, false);
            }
            col = 0;
        }
        let end = col + 1 + u16::from(wide);
        if end >= self.cols { (self.cols - 1, true) } else { (end, false) }
    }

    fn repeat(&mut self, out: Out<'_>, mut pending: bool, autowrap: bool, n: u16) {
        if let Some(wide) = self.previous {
            for _ in 0..n.max(1) {
                let (_, next) = self.after(self.col, pending, autowrap, wide);
                self.print(out, pending, autowrap, wide);
                pending = next;
            }
        }
    }

    fn repeat_lands_pending(&self, mut pending: bool, autowrap: bool, n: u16) -> bool {
        let Some(wide) = self.previous else { return false };
        let mut col = self.col;
        for _ in 0..n.max(1) {
            (col, pending) = self.after(col, pending, autowrap, wide);
        }
        pending
    }
}

type R = Result<(), ()>;
type G = Result<bool, ()>;

impl WrapStateMachineContext for Screen {
    fn fills(&self, _: Out<'_>, e: &Print) -> G {
        Ok(self.after(self.col, false, true, e.wide).1)
    }
    fn print(&mut self, out: Out<'_>, e: &Print) -> R {
        Ok(Screen::print(self, out, false, true, e.wide))
    }
    fn fills_after_wrap(&self, _: Out<'_>, e: &Print) -> G {
        Ok(self.after(self.col, true, true, e.wide).1)
    }
    fn wrap_print(&mut self, out: Out<'_>, e: &Print) -> R {
        Ok(Screen::print(self, out, true, true, e.wide))
    }
    fn fills_no_autowrap(&self, _: Out<'_>, e: &Print) -> G {
        Ok(self.after(self.col, false, false, e.wide).1)
    }
    fn print_no_autowrap(&mut self, out: Out<'_>, e: &Print) -> R {
        Ok(Screen::print(self, out, false, false, e.wide))
    }
    fn fills_again(&self, _: Out<'_>, e: &Print) -> G {
        Ok(self.after(self.col, true, false, e.wide).1)
    }
    fn overwrite(&mut self, out: Out<'_>, e: &Print) -> R {
        Ok(Screen::print(self, out, true, false, e.wide))
    }

    fn repeats(&self, _: Out<'_>, _: &Repeat) -> G {
        Ok(self.previous.is_some())
    }
    fn repeat_fills(&self, _: Out<'_>, e: &Repeat) -> G {
        Ok(self.repeat_lands_pending(false, true, e.0))
    }
    fn repeat(&mut self, out: Out<'_>, e: &Repeat) -> R {
        Ok(Screen::repeat(self, out, false, true, e.0))
    }
    fn repeat_fills_after_wrap(&self, _: Out<'_>, e: &Repeat) -> G {
        Ok(self.repeat_lands_pending(true, true, e.0))
    }
    fn repeat_wrapping(&mut self, out: Out<'_>, e: &Repeat) -> R {
        Ok(Screen::repeat(self, out, true, true, e.0))
    }
    fn repeat_fills_no_autowrap(&self, _: Out<'_>, e: &Repeat) -> G {
        Ok(self.repeat_lands_pending(false, false, e.0))
    }
    fn repeat_no_autowrap(&mut self, out: Out<'_>, e: &Repeat) -> R {
        Ok(Screen::repeat(self, out, false, false, e.0))
    }
    fn repeat_fills_again(&self, _: Out<'_>, e: &Repeat) -> G {
        Ok(self.repeat_lands_pending(true, false, e.0))
    }
    fn repeat_overwriting(&mut self, out: Out<'_>, e: &Repeat) -> R {
        Ok(Screen::repeat(self, out, true, false, e.0))
    }

    fn carriage_return(&mut self, _: Out<'_>, _: &CarriageReturn) -> R {
        self.col = 0;
        Ok(())
    }
    fn line_feed(&mut self, out: Out<'_>, _: &LineFeed) -> R {
        self.linefeed(out);
        Ok(())
    }
    fn backspace(&mut self, _: Out<'_>, _: &Backspace) -> R {
        self.col = self.col.saturating_sub(1);
        Ok(())
    }
    fn cursor_to(&mut self, _: Out<'_>, e: &CursorTo) -> R {
        self.row = e.row.min(self.rows - 1);
        self.col = e.col.min(self.cols - 1);
        Ok(())
    }
    fn erases(&self, _: Out<'_>, e: &EraseLine) -> G {
        Ok(e.0 <= 2)
    }
    fn erase_line(&mut self, out: Out<'_>, e: &EraseLine) -> R {
        if e.0 != 1 && self.links & (1 << self.row) != 0 {
            self.links &= !(1 << self.row);
            out.push(Effect::Unlink { row: self.row });
        }
        Ok(())
    }
    fn at_top(&self, _: Out<'_>, _: &ReverseIndex) -> G {
        Ok(self.row == 0)
    }
    fn scroll_down(&mut self, out: Out<'_>, _: &ReverseIndex) -> R {
        out.push(Effect::ScrollDown);
        let mask = if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 };
        self.links = (self.links << 1) & mask;
        Ok(())
    }
    fn up(&mut self, _: Out<'_>, _: &ReverseIndex) -> R {
        self.row -= 1;
        Ok(())
    }
    fn tab(&mut self, _: Out<'_>, _: &Tab) -> R {
        self.col = ((self.col / 8 + 1) * 8).min(self.cols - 1);
        Ok(())
    }

    fn turns_off(&self, _: Out<'_>, e: &SetAutowrap) -> G {
        Ok(!e.0)
    }
    fn turns_on(&self, _: Out<'_>, e: &SetAutowrap) -> G {
        Ok(e.0)
    }
    fn save_pending(&mut self, _: Out<'_>, _: &SaveCursor) -> R {
        self.saved = Saved { row: self.row, col: self.col, pending: true };
        Ok(())
    }
    fn save(&mut self, _: Out<'_>, _: &SaveCursor) -> R {
        self.saved = Saved { row: self.row, col: self.col, pending: false };
        Ok(())
    }
    fn saved_pending(&self, _: Out<'_>, _: &RestoreCursor) -> G {
        Ok(self.saved.pending)
    }
    fn restore(&mut self, _: Out<'_>, _: &RestoreCursor) -> R {
        self.row = self.saved.row;
        self.col = self.saved.col;
        Ok(())
    }
    fn reset(&mut self, _: Out<'_>, _: &FullReset) -> R {
        *self = Screen::new(self.cols, self.rows);
        Ok(())
    }
}

pub struct Wrap {
    /// The generated machine: its `process_event`, `is` and
    /// `visit_current_states` are the typed API.
    pub machine: WrapStateMachine<Screen>,
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self { machine: WrapStateMachine::new(Screen::new(config.cols, config.rows)) }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let m = &mut self.machine;
        // An event no row takes is an `Err`: ignored, as the spec says.
        let _ = match event {
            Event::Print { wide } => m.process_event(out, Print { wide }),
            Event::Repeat(n) => m.process_event(out, Repeat(n)),
            Event::CarriageReturn => m.process_event(out, CarriageReturn),
            Event::LineFeed => m.process_event(out, LineFeed),
            Event::Backspace => m.process_event(out, Backspace),
            Event::Tab => m.process_event(out, Tab),
            Event::CursorTo { row, col } => m.process_event(out, CursorTo { row, col }),
            Event::ReverseIndex => m.process_event(out, ReverseIndex),
            Event::EraseLine(n) => m.process_event(out, EraseLine(n)),
            Event::SetAutowrap(on) => m.process_event(out, SetAutowrap(on)),
            Event::SaveCursor => m.process_event(out, SaveCursor),
            Event::RestoreCursor => m.process_event(out, RestoreCursor),
            Event::FullReset => m.process_event(out, FullReset),
        };
    }

    fn state(&self) -> StateName {
        let mut name = StateName::Normal;
        self.machine.visit_current_states(|s| {
            name = match s {
                WrapStates::Normal => StateName::Normal,
                WrapStates::PendingWrap => StateName::PendingWrap,
                WrapStates::NormalNoAutowrap => StateName::NormalNoAutowrap,
                WrapStates::PendingNoAutowrap => StateName::PendingNoAutowrap,
            }
        });
        name
    }
}
