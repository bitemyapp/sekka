//! `wrap` with statum 0.9.0: four `#[state]`s (pending × autowrap), each
//! with a token only this module makes (they carry no data),
//! the cursor and the row links as the `#[machine] Screen`'s fields. Only
//! `Screen<Pending>` (autowrap on) soft-wraps a pending wrap, only a print
//! reaching the last column (or a restore of such a cursor) makes a pending
//! state, and the controls are split by whether they clear a pending wrap
//! (`Clearing`) or keep it (`Keeping`). `handle` matches `screen::SomeState`.

use crate::{SEAL, Seal};
use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};
use statum::{Branch, machine, state, transition};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, Default)]
struct Saved {
    row: u16,
    col: u16,
    pending: bool,
}

/// Controls that clear a pending wrap.
#[derive(Clone, Copy, Debug)]
pub enum Clearing {
    CarriageReturn,
    LineFeed,
    Backspace,
    CursorTo {
        row: u16,
        col: u16,
    },
    /// EL 0, 1 or 2.
    EraseLine(u8),
}

/// Controls that keep a pending wrap.
#[derive(Clone, Copy, Debug)]
pub enum Keeping {
    Tab,
    /// EL with a mode other than 0, 1, 2.
    EraseOther,
    Save,
}

tokens!(NormalTok, PendingTok, NormalNoWrapTok, PendingNoWrapTok);

#[state]
#[derive(Debug)]
pub enum WrapState {
    Normal(NormalTok),
    Pending(PendingTok),
    NormalNoWrap(NormalNoWrapTok),
    PendingNoWrap(PendingNoWrapTok),
}

/// The cursor and the row links, carried across every state.
#[machine]
#[derive(Debug)]
pub struct Screen<WrapState> {
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    links: u128,
    seal: Seal,
}

/// What a print or a restore leads to.
pub type Ready = ::statum::Branch<Screen<Normal>, Screen<Pending>>;
pub type ReadyNoWrap = ::statum::Branch<Screen<NormalNoWrap>, Screen<PendingNoWrap>>;

impl Screen<Normal> {
    #[inline]
    pub fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Screen::<Normal>::builder()
            .state_data(NormalTok(()))
            .cols(config.cols)
            .rows(config.rows)
            .row(0)
            .col(0)
            .saved(Saved::default())
            .previous(None)
            .links(0)
            .seal(SEAL)
            .build()
    }
}

/// What every state does to the cursor and the links.
impl<S: WrapStateTrait> Screen<S> {
    pub fn previous(&self) -> Option<bool> {
        self.previous
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
    /// Writes at the cursor (no wrap pending): whether it reached the last column.
    #[inline]
    fn put(&mut self, wide: bool, out: &mut impl Sink<Effect>) -> bool {
        out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col += if wide { 2 } else { 1 };
        if self.col >= self.cols {
            self.col = self.cols - 1;
            true
        } else {
            false
        }
    }
    fn clear_with(&mut self, op: Clearing, out: &mut impl Sink<Effect>) {
        match op {
            Clearing::CarriageReturn => self.col = 0,
            Clearing::LineFeed => self.linefeed(out),
            Clearing::Backspace => self.col = self.col.saturating_sub(1),
            Clearing::CursorTo { row, col } => {
                self.row = row.min(self.rows - 1);
                self.col = col.min(self.cols - 1);
            }
            Clearing::EraseLine(m) => {
                if m != 1 && self.links & (1 << self.row) != 0 {
                    self.links &= !(1 << self.row);
                    out.push(Effect::Unlink { row: self.row });
                }
            }
        }
    }
    fn keep_with(&mut self, op: Keeping, pending: bool) {
        match op {
            Keeping::Tab => {
                let last = self.cols - 1;
                let next = (self.col / 8 + 1) * 8;
                self.col = if next <= last { next } else { last };
            }
            Keeping::EraseOther => {}
            Keeping::Save => self.saved = Saved { row: self.row, col: self.col, pending },
        }
    }
    /// RI: whether it scrolled (at the top).
    fn scroll_back(&mut self, out: &mut impl Sink<Effect>) -> bool {
        if self.row == 0 {
            out.push(Effect::ScrollDown);
            let mask = if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 };
            self.links = (self.links << 1) & mask;
            true
        } else {
            self.row -= 1;
            false
        }
    }
    /// DECRC: whether the saved cursor had a wrap pending.
    fn restore_saved(&mut self) -> bool {
        self.row = self.saved.row;
        self.col = self.saved.col;
        self.saved.pending
    }
}

#[transition]
impl Screen<Normal> {
    #[inline]
    pub fn print(mut self, wide: bool, out: &mut impl Sink<Effect>) -> Ready {
        self.previous = Some(wide);
        if wide && self.col + 1 >= self.cols {
            out.push(Effect::SpacerHead { row: self.row, col: self.col });
            self.soft_wrap(out);
        }
        if self.put(wide, out) { Branch::Second(self.transition_with(PendingTok(()))) } else { Branch::First(self) }
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> Screen<Normal> {
        self.clear_with(op, out);
        self
    }
    pub fn keeping(mut self, op: Keeping) -> Screen<Normal> {
        self.keep_with(op, false);
        self
    }
    pub fn reverse_index(mut self, out: &mut impl Sink<Effect>) -> Screen<Normal> {
        self.scroll_back(out);
        self
    }
    pub fn restore(mut self) -> Ready {
        if self.restore_saved() { Branch::Second(self.transition_with(PendingTok(()))) } else { Branch::First(self) }
    }
    pub fn autowrap_off(self) -> Screen<NormalNoWrap> {
        self.transition_with(NormalNoWrapTok(()))
    }
}

#[transition]
impl Screen<Pending> {
    /// Wraps first (soft wrap, perhaps a scroll), then prints.
    #[inline]
    pub fn print(mut self, wide: bool, out: &mut impl Sink<Effect>) -> Ready {
        self.soft_wrap(out);
        self.transition_with(NormalTok(())).print(wide, out)
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> Screen<Normal> {
        self.clear_with(op, out);
        self.transition_with(NormalTok(()))
    }
    pub fn keeping(mut self, op: Keeping) -> Screen<Pending> {
        self.keep_with(op, true);
        self
    }
    /// At the top it scrolls and keeps the pending wrap; else it moves up and clears it.
    pub fn reverse_index(
        mut self,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Screen<Pending>, Screen<Normal>> {
        if self.scroll_back(out) { Ok(self) } else { Err(self.transition_with(NormalTok(()))) }
    }
    pub fn restore(mut self) -> Ready {
        if self.restore_saved() { Branch::Second(self) } else { Branch::First(self.transition_with(NormalTok(()))) }
    }
    pub fn autowrap_off(self) -> Screen<PendingNoWrap> {
        self.transition_with(PendingNoWrapTok(()))
    }
}

#[transition]
impl Screen<NormalNoWrap> {
    #[inline]
    pub fn print(mut self, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap {
        self.previous = Some(wide);
        if wide && self.col + 1 >= self.cols {
            return Branch::First(self);
        }
        if self.put(wide, out) {
            Branch::Second(self.transition_with(PendingNoWrapTok(())))
        } else {
            Branch::First(self)
        }
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> Screen<NormalNoWrap> {
        self.clear_with(op, out);
        self
    }
    pub fn keeping(mut self, op: Keeping) -> Screen<NormalNoWrap> {
        self.keep_with(op, false);
        self
    }
    pub fn reverse_index(mut self, out: &mut impl Sink<Effect>) -> Screen<NormalNoWrap> {
        self.scroll_back(out);
        self
    }
    pub fn restore(mut self) -> ReadyNoWrap {
        if self.restore_saved() {
            Branch::Second(self.transition_with(PendingNoWrapTok(())))
        } else {
            Branch::First(self)
        }
    }
    pub fn autowrap_on(self) -> Screen<Normal> {
        self.transition_with(NormalTok(()))
    }
}

#[transition]
impl Screen<PendingNoWrap> {
    /// No wrap without autowrap: the print overwrites the last column.
    #[inline]
    pub fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap {
        self.transition_with(NormalNoWrapTok(())).print(wide, out)
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> Screen<NormalNoWrap> {
        self.clear_with(op, out);
        self.transition_with(NormalNoWrapTok(()))
    }
    pub fn keeping(mut self, op: Keeping) -> Screen<PendingNoWrap> {
        self.keep_with(op, true);
        self
    }
    pub fn reverse_index(
        mut self,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Screen<PendingNoWrap>, Screen<NormalNoWrap>> {
        if self.scroll_back(out) { Ok(self) } else { Err(self.transition_with(NormalNoWrapTok(()))) }
    }
    pub fn restore(mut self) -> ReadyNoWrap {
        if self.restore_saved() {
            Branch::Second(self)
        } else {
            Branch::First(self.transition_with(NormalNoWrapTok(())))
        }
    }
    pub fn autowrap_on(self) -> Screen<Pending> {
        self.transition_with(PendingTok(()))
    }
}

#[inline]
fn ready(r: Ready) -> screen::SomeState {
    match r {
        Branch::First(s) => screen::SomeState::Normal(s),
        Branch::Second(s) => screen::SomeState::Pending(s),
    }
}

#[inline]
fn ready_no_wrap(r: ReadyNoWrap) -> screen::SomeState {
    match r {
        Branch::First(s) => screen::SomeState::NormalNoWrap(s),
        Branch::Second(s) => screen::SomeState::PendingNoWrap(s),
    }
}

#[inline]
fn print(s: screen::SomeState, wide: bool, out: &mut impl Sink<Effect>) -> screen::SomeState {
    use screen::SomeState as S;
    match s {
        S::Normal(s) => ready(s.print(wide, out)),
        S::Pending(s) => ready(s.print(wide, out)),
        S::NormalNoWrap(s) => ready_no_wrap(s.print(wide, out)),
        S::PendingNoWrap(s) => ready_no_wrap(s.print(wide, out)),
    }
}

#[inline]
fn previous(s: &screen::SomeState) -> Option<bool> {
    use screen::SomeState as S;
    match s {
        S::Normal(s) => s.previous(),
        S::Pending(s) => s.previous(),
        S::NormalNoWrap(s) => s.previous(),
        S::PendingNoWrap(s) => s.previous(),
    }
}

pub struct Wrap {
    config: Config,
    /// `None` only while `handle` has moved the state out (statum's builder
    /// makes no cheap placeholder).
    state: Option<screen::SomeState>,
}

impl Wrap {
    pub fn typed(&self) -> &screen::SomeState {
        self.state.as_ref().expect("a state")
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        Self { config, state: Some(screen::SomeState::Normal(Screen::<Normal>::new(config))) }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use screen::SomeState as S;
        let state = self.state.take().expect("a state");
        let clearing = match event {
            Event::CarriageReturn => Some(Clearing::CarriageReturn),
            Event::LineFeed => Some(Clearing::LineFeed),
            Event::Backspace => Some(Clearing::Backspace),
            Event::CursorTo { row, col } => Some(Clearing::CursorTo { row, col }),
            Event::EraseLine(m @ 0..=2) => Some(Clearing::EraseLine(m)),
            _ => None,
        };
        let keeping = match event {
            Event::Tab => Some(Keeping::Tab),
            Event::EraseLine(_) => Some(Keeping::EraseOther),
            Event::SaveCursor => Some(Keeping::Save),
            _ => None,
        };
        self.state = Some(match (state, event) {
            (s, Event::Print { wide }) => print(s, wide, out),
            (s, Event::Repeat(n)) => match previous(&s) {
                Some(wide) => {
                    let mut s = s;
                    for _ in 0..n.max(1) {
                        s = print(s, wide, out);
                    }
                    s
                }
                None => s,
            },
            (_, Event::FullReset) => S::Normal(Screen::<Normal>::new(self.config)),
            (S::Normal(s), Event::SetAutowrap(false)) => S::NormalNoWrap(s.autowrap_off()),
            (S::Pending(s), Event::SetAutowrap(false)) => S::PendingNoWrap(s.autowrap_off()),
            (S::NormalNoWrap(s), Event::SetAutowrap(true)) => S::Normal(s.autowrap_on()),
            (S::PendingNoWrap(s), Event::SetAutowrap(true)) => S::Pending(s.autowrap_on()),
            (s, Event::SetAutowrap(_)) => s,
            (S::Normal(s), Event::ReverseIndex) => S::Normal(s.reverse_index(out)),
            (S::NormalNoWrap(s), Event::ReverseIndex) => S::NormalNoWrap(s.reverse_index(out)),
            (S::Pending(s), Event::ReverseIndex) => match s.reverse_index(out) {
                Ok(p) => S::Pending(p),
                Err(n) => S::Normal(n),
            },
            (S::PendingNoWrap(s), Event::ReverseIndex) => match s.reverse_index(out) {
                Ok(p) => S::PendingNoWrap(p),
                Err(n) => S::NormalNoWrap(n),
            },
            (S::Normal(s), Event::RestoreCursor) => ready(s.restore()),
            (S::Pending(s), Event::RestoreCursor) => ready(s.restore()),
            (S::NormalNoWrap(s), Event::RestoreCursor) => ready_no_wrap(s.restore()),
            (S::PendingNoWrap(s), Event::RestoreCursor) => ready_no_wrap(s.restore()),
            (s, _) => match (s, clearing, keeping) {
                (S::Normal(s), Some(op), _) => S::Normal(s.clearing(op, out)),
                (S::Pending(s), Some(op), _) => S::Normal(s.clearing(op, out)),
                (S::NormalNoWrap(s), Some(op), _) => S::NormalNoWrap(s.clearing(op, out)),
                (S::PendingNoWrap(s), Some(op), _) => S::NormalNoWrap(s.clearing(op, out)),
                (S::Normal(s), None, Some(op)) => S::Normal(s.keeping(op)),
                (S::Pending(s), None, Some(op)) => S::Pending(s.keeping(op)),
                (S::NormalNoWrap(s), None, Some(op)) => S::NormalNoWrap(s.keeping(op)),
                (S::PendingNoWrap(s), None, Some(op)) => S::PendingNoWrap(s.keeping(op)),
                (s, None, None) => s,
            },
        })
    }

    fn state(&self) -> StateName {
        use screen::SomeState as S;
        match self.typed() {
            S::Normal(_) => StateName::Normal,
            S::Pending(_) => StateName::PendingWrap,
            S::NormalNoWrap(_) => StateName::NormalNoAutowrap,
            S::PendingNoWrap(_) => StateName::PendingNoAutowrap,
        }
    }
}
