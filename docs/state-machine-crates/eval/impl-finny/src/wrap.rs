//! `wrap` with finny 0.3.0: two orthogonal regions, the pending wrap
//! (`Normal`, `Pending`) and the mode (`Autowrap`, `NoAutowrap`), so
//! `SetAutowrap` keeps the pending flag without history. The cursor, saved
//! cursor, REP memory and row links are the shared context. finny picks a
//! transition (and so the next state) before its action runs, so the print
//! guards predict where the print lands (`lands_pending`); and a guard can't
//! see the other region's state, so the mode region's entry actions mirror
//! it into `Screen::autowrap`. `Repeat(n)` re-queues one `Print` and the
//! rest of the repeat (run to completion; `handle` drains the effects after
//! each queued event, so the buffer stays one print long).

use finny::decl::{BuiltFsm, FsmBuilder};
use finny::inspect::null::InspectNull;
use finny::{
    FsmCurrentState, FsmEvent, FsmEventQueue, FsmEventQueueArray, FsmEventQueueSender, FsmFactory, FsmResult,
    FsmTimersNull, finny_fsm,
};
use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

use crate::{Effects, Front};

pub const IMPLEMENTED: bool = true;

// Events.
#[derive(Clone, Copy, Debug)]
pub struct Print {
    pub wide: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Repeat(pub u16);
#[derive(Clone, Copy, Debug)]
pub struct CarriageReturn;
#[derive(Clone, Copy, Debug)]
pub struct LineFeed;
#[derive(Clone, Copy, Debug)]
pub struct Backspace;
#[derive(Clone, Copy, Debug)]
pub struct Tab;
#[derive(Clone, Copy, Debug)]
pub struct CursorTo {
    pub row: u16,
    pub col: u16,
}
#[derive(Clone, Copy, Debug)]
pub struct ReverseIndex;
#[derive(Clone, Copy, Debug)]
pub struct EraseLine(pub u8);
#[derive(Clone, Copy, Debug)]
pub struct SetAutowrap(pub bool);
#[derive(Clone, Copy, Debug)]
pub struct SaveCursor;
#[derive(Clone, Copy, Debug)]
pub struct RestoreCursor;
#[derive(Clone, Copy, Debug)]
pub struct FullReset;

// The pending region.
#[derive(Default)]
pub struct Normal;
#[derive(Default)]
pub struct Pending;
// The mode region.
#[derive(Default)]
pub struct Autowrap;
#[derive(Default)]
pub struct NoAutowrap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub row: u16,
    pub col: u16,
    pub pending: bool,
}

/// The machine data (every state) and the effect buffer.
pub struct Screen {
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    /// Bit `r`: row `r` is soft-wrapped into row `r + 1`.
    links: u128,
    /// The mode region's state, mirrored for the pending region's guards.
    autowrap: bool,
    out: Effects<Effect, 8>,
}

impl Screen {
    pub fn row(&self) -> u16 {
        self.row
    }

    pub fn col(&self) -> u16 {
        self.col
    }

    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self {
            cols: config.cols,
            rows: config.rows,
            row: 0,
            col: 0,
            saved: Saved::default(),
            previous: None,
            links: 0,
            autowrap: true,
            out: Effects::new(Effect::ScrollUp),
        }
    }

    fn reset(&mut self) {
        self.row = 0;
        self.col = 0;
        self.saved = Saved::default();
        self.previous = None;
        self.links = 0;
    }

    fn mask(&self) -> u128 {
        if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 }
    }

    /// Without the pending flag, which is the region's state.
    fn linefeed(&mut self) {
        if self.row == self.rows - 1 {
            self.out.push(Effect::ScrollUp);
            self.links >>= 1;
        } else {
            self.row += 1;
        }
    }

    fn soft_wrap(&mut self) {
        self.out.push(Effect::SoftWrap { row: self.row });
        self.links |= 1 << self.row;
        self.linefeed();
        self.col = 0;
    }

    /// Whether `print(wide)` from here ends with a wrap pending: the guard
    /// of the transitions into (or staying in) `Pending`.
    #[inline]
    fn lands_pending(&self, wide: bool, pending: bool) -> bool {
        let mut col = if pending && self.autowrap { 0 } else { self.col };
        if wide && col + 1 >= self.cols {
            if !self.autowrap {
                return false;
            }
            col = 0;
        }
        col + if wide { 2 } else { 1 } >= self.cols
    }

    #[inline]
    fn print(&mut self, wide: bool, pending: bool) {
        self.previous = Some(wide);
        if pending && self.autowrap {
            self.soft_wrap();
        }
        if wide && self.col + 1 >= self.cols {
            if !self.autowrap {
                return;
            }
            self.out.push(Effect::SpacerHead { row: self.row, col: self.col });
            self.soft_wrap();
        }
        self.out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col += if wide { 2 } else { 1 };
        if self.col >= self.cols {
            self.col = self.cols - 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.row == 0 {
            self.out.push(Effect::ScrollDown);
            self.links = (self.links << 1) & self.mask();
        } else {
            self.row -= 1;
        }
    }

    fn erase_line(&mut self, m: u8) {
        if m != 1 && self.links & (1 << self.row) != 0 {
            self.links &= !(1 << self.row);
            self.out.push(Effect::Unlink { row: self.row });
        }
    }

    fn tab(&mut self) {
        let last = self.cols - 1;
        let next = (self.col / 8 + 1) * 8;
        self.col = if next <= last { next } else { last };
    }

    fn cursor_to(&mut self, ev: &CursorTo) {
        self.row = ev.row.min(self.rows - 1);
        self.col = ev.col.min(self.cols - 1);
    }

    fn restore(&mut self) {
        self.row = self.saved.row;
        self.col = self.saved.col;
    }
}

#[finny_fsm]
fn cursor_fsm(mut fsm: FsmBuilder<Cursor, Screen>) -> BuiltFsm {
    fsm.initial_states::<(Normal, Autowrap)>();

    // The pending region.
    fsm.state::<Normal>()
        .on_event::<Print>()
        .transition_to::<Pending>()
        .guard(|ev, ctx, _s| ctx.lands_pending(ev.wide, false))
        .action(|ev, ctx, _from, _to| ctx.print(ev.wide, false));
    fsm.state::<Normal>().on_event::<Print>().internal_transition().action(|ev, ctx, _s| ctx.print(ev.wide, false));
    fsm.state::<Pending>()
        .on_event::<Print>()
        .internal_transition()
        .guard(|ev, ctx, _s| ctx.lands_pending(ev.wide, true))
        .action(|ev, ctx, _s| ctx.print(ev.wide, true));
    fsm.state::<Pending>()
        .on_event::<Print>()
        .transition_to::<Normal>()
        .action(|ev, ctx, _from, _to| ctx.print(ev.wide, true));

    fsm.state::<Normal>()
        .on_event::<Repeat>()
        .internal_transition()
        .guard(|_ev, ctx, _s| ctx.previous.is_some())
        .action(|ev, ctx, _s| {
            // One print now, then the rest: never more than two queued.
            let wide = ctx.previous.unwrap();
            let _ = ctx.queue.enqueue(Print { wide });
            if ev.0 > 1 {
                let _ = ctx.queue.enqueue(Repeat(ev.0 - 1));
            }
        });
    fsm.state::<Pending>()
        .on_event::<Repeat>()
        .internal_transition()
        .guard(|_ev, ctx, _s| ctx.previous.is_some())
        .action(|ev, ctx, _s| {
            let wide = ctx.previous.unwrap();
            let _ = ctx.queue.enqueue(Print { wide });
            if ev.0 > 1 {
                let _ = ctx.queue.enqueue(Repeat(ev.0 - 1));
            }
        });

    fsm.state::<Normal>().on_event::<CarriageReturn>().internal_transition().action(|_ev, ctx, _s| ctx.col = 0);
    fsm.state::<Pending>().on_event::<CarriageReturn>().transition_to::<Normal>().action(|_ev, ctx, _f, _t| {
        ctx.col = 0;
    });
    fsm.state::<Normal>().on_event::<LineFeed>().internal_transition().action(|_ev, ctx, _s| ctx.linefeed());
    fsm.state::<Pending>().on_event::<LineFeed>().transition_to::<Normal>().action(|_ev, ctx, _f, _t| ctx.linefeed());
    fsm.state::<Normal>().on_event::<Backspace>().internal_transition().action(|_ev, ctx, _s| {
        ctx.col = ctx.col.saturating_sub(1);
    });
    fsm.state::<Pending>().on_event::<Backspace>().transition_to::<Normal>().action(|_ev, ctx, _f, _t| {
        ctx.col = ctx.col.saturating_sub(1);
    });
    // HT keeps the pending wrap.
    fsm.state::<Normal>().on_event::<Tab>().internal_transition().action(|_ev, ctx, _s| ctx.tab());
    fsm.state::<Pending>().on_event::<Tab>().internal_transition().action(|_ev, ctx, _s| ctx.tab());
    fsm.state::<Normal>().on_event::<CursorTo>().internal_transition().action(|ev, ctx, _s| ctx.cursor_to(ev));
    fsm.state::<Pending>().on_event::<CursorTo>().transition_to::<Normal>().action(|ev, ctx, _f, _t| ctx.cursor_to(ev));
    fsm.state::<Normal>().on_event::<ReverseIndex>().internal_transition().action(|_ev, ctx, _s| ctx.reverse_index());
    // At the top RI scrolls and keeps the pending wrap.
    fsm.state::<Pending>()
        .on_event::<ReverseIndex>()
        .internal_transition()
        .guard(|_ev, ctx, _s| ctx.row == 0)
        .action(|_ev, ctx, _s| ctx.reverse_index());
    fsm.state::<Pending>().on_event::<ReverseIndex>().transition_to::<Normal>().action(|_ev, ctx, _f, _t| {
        ctx.reverse_index();
    });
    fsm.state::<Normal>()
        .on_event::<EraseLine>()
        .internal_transition()
        .guard(|ev, _ctx, _s| ev.0 <= 2)
        .action(|ev, ctx, _s| ctx.erase_line(ev.0));
    fsm.state::<Pending>()
        .on_event::<EraseLine>()
        .transition_to::<Normal>()
        .guard(|ev, _ctx, _s| ev.0 <= 2)
        .action(|ev, ctx, _f, _t| ctx.erase_line(ev.0));
    fsm.state::<Normal>().on_event::<SaveCursor>().internal_transition().action(|_ev, ctx, _s| {
        ctx.saved = Saved { row: ctx.row, col: ctx.col, pending: false };
    });
    fsm.state::<Pending>().on_event::<SaveCursor>().internal_transition().action(|_ev, ctx, _s| {
        ctx.saved = Saved { row: ctx.row, col: ctx.col, pending: true };
    });
    fsm.state::<Normal>()
        .on_event::<RestoreCursor>()
        .transition_to::<Pending>()
        .guard(|_ev, ctx, _s| ctx.saved.pending)
        .action(|_ev, ctx, _f, _t| ctx.restore());
    fsm.state::<Normal>().on_event::<RestoreCursor>().internal_transition().action(|_ev, ctx, _s| ctx.restore());
    fsm.state::<Pending>()
        .on_event::<RestoreCursor>()
        .internal_transition()
        .guard(|_ev, ctx, _s| ctx.saved.pending)
        .action(|_ev, ctx, _s| ctx.restore());
    fsm.state::<Pending>().on_event::<RestoreCursor>().transition_to::<Normal>().action(|_ev, ctx, _f, _t| {
        ctx.restore();
    });
    fsm.state::<Normal>().on_event::<FullReset>().internal_transition().action(|_ev, ctx, _s| ctx.reset());
    fsm.state::<Pending>().on_event::<FullReset>().transition_to::<Normal>().action(|_ev, ctx, _f, _t| ctx.reset());

    // The mode region.
    fsm.state::<Autowrap>().on_entry(|_s, ctx| ctx.autowrap = true);
    fsm.state::<NoAutowrap>().on_entry(|_s, ctx| ctx.autowrap = false);
    fsm.state::<Autowrap>().on_event::<SetAutowrap>().transition_to::<NoAutowrap>().guard(|ev, _ctx, _s| !ev.0);
    fsm.state::<NoAutowrap>().on_event::<SetAutowrap>().transition_to::<Autowrap>().guard(|ev, _ctx, _s| ev.0);
    fsm.state::<NoAutowrap>().on_event::<FullReset>().transition_to::<Autowrap>();

    fsm.build()
}

pub struct Wrap {
    fsm: Front<Cursor, 2>,
}

impl Wrap {
    pub fn fsm(&self) -> &Front<Cursor, 2> {
        &self.fsm
    }

    /// finny's `dispatch` of a typed event with the queue run by hand,
    /// draining the effects after each queued event (a `Repeat(n)` emits up
    /// to 6n): the first event's result.
    #[inline]
    pub fn dispatch(&mut self, event: impl Into<CursorEvents>, out: &mut impl Sink<Effect>) -> FsmResult<()> {
        let r = self.fsm.dispatch_single_event(FsmEvent::Event(event.into()));
        self.fsm.context.out.drain(out);
        while let Some(ev) = self.fsm.queue.dequeue() {
            let _ = self.fsm.dispatch_single_event(FsmEvent::Event(ev));
            self.fsm.context.out.drain(out);
        }
        r
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        let mut fsm =
            Cursor::new_with(Screen::new(config), FsmEventQueueArray::new(), InspectNull::new(), FsmTimersNull)
                .expect("the states are built from defaults");
        fsm.start().expect("the initial states' entry actions only set the mode");
        Self { fsm }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let ev: CursorEvents = match event {
            Event::Print { wide } => Print { wide }.into(),
            Event::Repeat(n) => Repeat(n).into(),
            Event::CarriageReturn => CarriageReturn.into(),
            Event::LineFeed => LineFeed.into(),
            Event::Backspace => Backspace.into(),
            Event::Tab => Tab.into(),
            Event::CursorTo { row, col } => CursorTo { row, col }.into(),
            Event::ReverseIndex => ReverseIndex.into(),
            Event::EraseLine(m) => EraseLine(m).into(),
            Event::SetAutowrap(on) => SetAutowrap(on).into(),
            Event::SaveCursor => SaveCursor.into(),
            Event::RestoreCursor => RestoreCursor.into(),
            Event::FullReset => FullReset.into(),
        };
        // `Err(NoTransition)`: an event both regions ignore.
        let _ = self.dispatch(ev, out);
    }

    fn state(&self) -> StateName {
        use FsmCurrentState::State;
        let [wrap, mode] = self.fsm.get_current_states();
        let pending = wrap == State(CursorCurrentState::Pending);
        match (pending, mode == State(CursorCurrentState::NoAutowrap)) {
            (false, false) => StateName::Normal,
            (true, false) => StateName::PendingWrap,
            (false, true) => StateName::NormalNoAutowrap,
            (true, true) => StateName::PendingNoAutowrap,
        }
    }
}
