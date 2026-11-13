//! `wrap` with state-machines 0.34.0: the two flags are two superstates
//! (`Autowrap`, `NoAutowrap`) of two leaves each; the cursor, the saved
//! cursor, REP's memory and the row links are the context. Guards choose
//! the next leaf from the context before the callbacks change it, so
//! `ends_pending` predicts what `print` will do.

use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};
use state_machines::state_machine;

pub const IMPLEMENTED: bool = true;

pub use machine::{
    Autowrap, Mode, NoAutowrap, Normal, NormalNoAutowrap, PendingNoAutowrap, PendingWrap, Screen,
    ScreenNormalNoAutowrapPrintOutcome, ScreenNormalNoAutowrapRestoreCursorOutcome,
    ScreenNormalNoAutowrapReverseIndexOutcome, ScreenNormalNoAutowrapSetAutowrapOutcome, ScreenNormalPrintOutcome,
    ScreenNormalRestoreCursorOutcome, ScreenNormalReverseIndexOutcome, ScreenNormalSetAutowrapOutcome,
    ScreenPendingNoAutowrapPrintOutcome, ScreenPendingNoAutowrapRestoreCursorOutcome,
    ScreenPendingNoAutowrapReverseIndexOutcome, ScreenPendingNoAutowrapSetAutowrapOutcome,
    ScreenPendingWrapPrintOutcome, ScreenPendingWrapRestoreCursorOutcome, ScreenPendingWrapReverseIndexOutcome,
    ScreenPendingWrapSetAutowrapOutcome, Wrap,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub row: u16,
    pub col: u16,
    pub pending: bool,
}

/// The context: the screen size, the cursor and the row links.
#[derive(Debug)]
pub struct Ctx {
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    /// Bit `r`: row `r` is soft-wrapped into row `r + 1`.
    links: u128,
    out: Vec<Effect>,
}

impl Ctx {
    pub fn new(config: Config) -> Self {
        Self::with_buffer(config, Vec::with_capacity(crate::out::CAPACITY))
    }

    fn with_buffer(config: Config, out: Vec<Effect>) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self {
            cols: config.cols,
            rows: config.rows,
            row: 0,
            col: 0,
            saved: Saved::default(),
            previous: None,
            links: 0,
            out,
        }
    }

    fn mask(&self) -> u128 {
        if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 }
    }

    #[inline]
    fn linefeed(&mut self) {
        if self.row == self.rows - 1 {
            self.out.push(Effect::ScrollUp);
            self.links >>= 1;
        } else {
            self.row += 1;
        }
    }

    #[cold]
    fn soft_wrap(&mut self) {
        self.out.push(Effect::SoftWrap { row: self.row });
        self.links |= 1 << self.row;
        self.linefeed();
        self.col = 0;
    }

    /// Whether print(wide) from (`pending`, `autowrap`) leaves a wrap pending.
    #[inline]
    fn ends_pending(&self, wide: bool, pending: bool, autowrap: bool) -> bool {
        let mut col = if pending && autowrap { 0 } else { self.col };
        if wide && col + 1 >= self.cols {
            if !autowrap {
                return false;
            }
            col = 0;
        }
        col + if wide { 2 } else { 1 } >= self.cols
    }

    #[inline]
    fn print(&mut self, wide: bool, pending: bool, autowrap: bool) {
        self.previous = Some(wide);
        if pending && autowrap {
            self.soft_wrap();
        }
        if wide && self.col + 1 >= self.cols {
            if !autowrap {
                return;
            }
            self.out.push(Effect::SpacerHead { row: self.row, col: self.col });
            self.soft_wrap();
        }
        self.out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col = (self.col + if wide { 2 } else { 1 }).min(self.cols - 1);
    }
}

/// The generated machines, their callbacks and the facade. The module is
/// private so that the generated `Dynamic*` wrapper (with its
/// `new_init_state`), its event and its state enums aren't exported.
mod machine {
    use super::*;
    use crate::out::drain;

    state_machine! {
        name: Screen,
        context: Ctx,
        dynamic: true,
        initial: Normal,
        states: [
            superstate Autowrap {
                state Normal,
                state PendingWrap,
            },
            superstate NoAutowrap {
                state NormalNoAutowrap,
                state PendingNoAutowrap,
            },
        ],
        events {
            // `None`: REP's previous scalar.
            print {
                payload: Option<bool>,
                guards: [has_scalar],
                branching: true,
                transition: { from: Autowrap, to: PendingWrap, guards: [ends_pending], before: [print_cell] }
                transition: { from: Autowrap, to: Normal, fallback: true, before: [print_cell] }
                transition: { from: NoAutowrap, to: PendingNoAutowrap, guards: [ends_pending], before: [print_cell] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, fallback: true, before: [print_cell] }
            }
            carriage_return {
                transition: { from: Autowrap, to: Normal, before: [back_to_column_0] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, before: [back_to_column_0] }
            }
            line_feed {
                transition: { from: Autowrap, to: Normal, before: [down_a_row] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, before: [down_a_row] }
            }
            backspace {
                transition: { from: Autowrap, to: Normal, before: [back_a_column] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, before: [back_a_column] }
            }
            // Keeps the pending wrap.
            tab {
                transition: { from: [Autowrap, NoAutowrap], internal: true, before: [next_tab_stop] }
            }
            cursor_to {
                payload: (u16, u16),
                transition: { from: Autowrap, to: Normal, before: [move_to] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, before: [move_to] }
            }
            reverse_index {
                branching: true,
                transition: { from: [Autowrap, NoAutowrap], internal: true, guards: [at_top], before: [scroll_down] }
                transition: { from: Autowrap, to: Normal, fallback: true, before: [up_a_row] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, fallback: true, before: [up_a_row] }
            }
            // Modes above 2 are ignored (the guard refuses them).
            erase_line {
                payload: u8,
                transition: { from: Autowrap, to: Normal, guards: [erases], before: [erase] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, guards: [erases], before: [erase] }
            }
            set_autowrap {
                payload: bool,
                branching: true,
                transition: { from: [Normal, NormalNoAutowrap], to: Normal, guards: [turns_on] }
                transition: { from: [Normal, NormalNoAutowrap], to: NormalNoAutowrap, fallback: true }
                transition: { from: [PendingWrap, PendingNoAutowrap], to: PendingWrap, guards: [turns_on] }
                transition: { from: [PendingWrap, PendingNoAutowrap], to: PendingNoAutowrap, fallback: true }
            }
            save_cursor {
                transition: { from: [Autowrap, NoAutowrap], internal: true, before: [save] }
            }
            restore_cursor {
                branching: true,
                transition: { from: Autowrap, to: PendingWrap, guards: [saved_pending], before: [restore] }
                transition: { from: Autowrap, to: Normal, fallback: true, before: [restore] }
                transition: { from: NoAutowrap, to: PendingNoAutowrap, guards: [saved_pending], before: [restore] }
                transition: { from: NoAutowrap, to: NormalNoAutowrap, fallback: true, before: [restore] }
            }
            full_reset {
                transition: { from: [Autowrap, NoAutowrap], to: Normal, before: [reset] }
            }
        }
    }

    /// The two flags of each leaf, so one callback serves all four: the
    /// generated transition methods live on `Screen<Leaf>`, where these bounds hold.
    pub trait Mode {
        const PENDING: bool;
        const AUTOWRAP: bool;
    }

    impl Mode for Normal {
        const PENDING: bool = false;
        const AUTOWRAP: bool = true;
    }
    impl Mode for PendingWrap {
        const PENDING: bool = true;
        const AUTOWRAP: bool = true;
    }
    impl Mode for NormalNoAutowrap {
        const PENDING: bool = false;
        const AUTOWRAP: bool = false;
    }
    impl Mode for PendingNoAutowrap {
        const PENDING: bool = true;
        const AUTOWRAP: bool = false;
    }

    impl<S: Mode> Screen<S> {
        fn has_scalar(&self, ctx: &Ctx, scalar: &Option<bool>) -> bool {
            scalar.or(ctx.previous).is_some()
        }

        fn ends_pending(&self, ctx: &Ctx, scalar: &Option<bool>) -> bool {
            let wide = scalar.or(ctx.previous).unwrap_or(false);
            ctx.ends_pending(wide, S::PENDING, S::AUTOWRAP)
        }

        fn print_cell(&mut self, scalar: &Option<bool>) {
            let wide = scalar.or(self.ctx.previous).expect("guarded by has_scalar");
            self.ctx.print(wide, S::PENDING, S::AUTOWRAP);
        }

        fn save(&mut self) {
            self.ctx.saved = Saved { row: self.ctx.row, col: self.ctx.col, pending: S::PENDING };
        }
    }

    impl<S> Screen<S> {
        fn back_to_column_0(&mut self) {
            self.ctx.col = 0;
        }

        fn down_a_row(&mut self) {
            self.ctx.linefeed();
        }

        fn back_a_column(&mut self) {
            self.ctx.col = self.ctx.col.saturating_sub(1);
        }

        fn next_tab_stop(&mut self) {
            let last = self.ctx.cols - 1;
            self.ctx.col = ((self.ctx.col / 8 + 1) * 8).min(last);
        }

        fn move_to(&mut self, &(row, col): &(u16, u16)) {
            self.ctx.row = row.min(self.ctx.rows - 1);
            self.ctx.col = col.min(self.ctx.cols - 1);
        }

        fn at_top(&self, ctx: &Ctx) -> bool {
            ctx.row == 0
        }

        fn scroll_down(&mut self) {
            self.ctx.out.push(Effect::ScrollDown);
            self.ctx.links = (self.ctx.links << 1) & self.ctx.mask();
        }

        fn up_a_row(&mut self) {
            self.ctx.row -= 1;
        }

        fn erases(&self, _: &Ctx, &mode: &u8) -> bool {
            mode <= 2
        }

        fn erase(&mut self, &mode: &u8) {
            let ctx = &mut self.ctx;
            if mode != 1 && ctx.links & (1 << ctx.row) != 0 {
                ctx.links &= !(1 << ctx.row);
                ctx.out.push(Effect::Unlink { row: ctx.row });
            }
        }

        fn turns_on(&self, _: &Ctx, &on: &bool) -> bool {
            on
        }

        fn saved_pending(&self, ctx: &Ctx) -> bool {
            ctx.saved.pending
        }

        fn restore(&mut self) {
            self.ctx.row = self.ctx.saved.row;
            self.ctx.col = self.ctx.saved.col;
        }

        fn reset(&mut self) {
            // Everything back to the initial values, but keep the buffer.
            let config = Config { cols: self.ctx.cols, rows: self.ctx.rows };
            let out = std::mem::take(&mut self.ctx.out);
            self.ctx = Ctx::with_buffer(config, out);
        }
    }

    /// The `Machine` facade over the generated runtime wrapper.
    pub struct Wrap {
        machine: DynamicScreen,
    }

    /// The context inside the wrapper (`None` once a panicking callback
    /// poisoned it). `inner` and `AnyScreenState` are private generated
    /// items, visible here because the macro expanded in this module.
    impl DynamicScreen {
        fn ctx(&mut self) -> Option<&mut Ctx> {
            Some(match self.inner.as_mut()? {
                AnyScreenState::Normal(m) => &mut m.ctx,
                AnyScreenState::PendingWrap(m) => &mut m.ctx,
                AnyScreenState::NormalNoAutowrap(m) => &mut m.ctx,
                AnyScreenState::PendingNoAutowrap(m) => &mut m.ctx,
            })
        }
    }

    impl<S> Screen<S> {
        /// Takes the effects the callbacks buffered, for driving the typed
        /// machine directly (the compile-fail probes do).
        pub fn take_effects(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.ctx.out)
        }
    }

    impl Machine for Wrap {
        fn new(config: Config) -> Self {
            Self { machine: DynamicScreen::new(Ctx::new(config)) }
        }

        #[inline]
        fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
            let event = match event {
                Event::Print { wide } => ScreenEvent::Print(Some(wide)),
                // One event is one transition, and each printed cell may change
                // the state: REP is that many prints of the previous scalar.
                Event::Repeat(n) => {
                    for _ in 0..n.max(1) {
                        if self.machine.handle(ScreenEvent::Print(None)).is_err() {
                            break;
                        }
                    }
                    if let Some(ctx) = self.machine.ctx() {
                        drain(&mut ctx.out, out);
                    }
                    return;
                }
                Event::CarriageReturn => ScreenEvent::CarriageReturn,
                Event::LineFeed => ScreenEvent::LineFeed,
                Event::Backspace => ScreenEvent::Backspace,
                Event::Tab => ScreenEvent::Tab,
                Event::CursorTo { row, col } => ScreenEvent::CursorTo((row, col)),
                Event::ReverseIndex => ScreenEvent::ReverseIndex,
                Event::EraseLine(m) => ScreenEvent::EraseLine(m),
                Event::SetAutowrap(on) => ScreenEvent::SetAutowrap(on),
                Event::SaveCursor => ScreenEvent::SaveCursor,
                Event::RestoreCursor => ScreenEvent::RestoreCursor,
                Event::FullReset => ScreenEvent::FullReset,
            };
            // `Err`: a guard refused (EL above 2, REP with nothing to repeat).
            let _ = self.machine.handle(event);
            if let Some(ctx) = self.machine.ctx() {
                drain(&mut ctx.out, out);
            }
        }

        fn state(&self) -> StateName {
            match self.machine.current_state() {
                ScreenState::Normal => StateName::Normal,
                ScreenState::PendingWrap => StateName::PendingWrap,
                ScreenState::NormalNoAutowrap => StateName::NormalNoAutowrap,
                ScreenState::PendingNoAutowrap => StateName::PendingNoAutowrap,
            }
        }
    }
}
