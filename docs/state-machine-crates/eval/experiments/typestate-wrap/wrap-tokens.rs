//! `wrap` as hand-written typestate over a borrowed context: `Normal`,
//! `Pending`, `NormalNoWrap`, `PendingNoWrap` are zero-sized state tokens,
//! and every transition takes the screen context as `&mut Screen`. Only `Pending` (autowrap
//! on) can soft-wrap a pending wrap, only a print reaching the last column
//! makes a pending state, and the controls are split by whether they clear
//! a pending wrap (`Clearing`) or keep it (`Keeping`).

use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, Default)]
struct Saved {
    row: u16,
    col: u16,
    pending: bool,
}

/// The cursor and the row links, shared by every state.
#[derive(Clone, Debug)]
pub struct Screen {
    cols: u16,
    rows: u16,
    row: u16,
    col: u16,
    saved: Saved,
    previous: Option<bool>,
    links: u128,
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


impl Screen {
    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Screen {
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
    fn clearing(&mut self, op: Clearing, out: &mut impl Sink<Effect>) {
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
    fn keeping(&mut self, op: Keeping, pending: bool) {
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
    fn reverse_index(&mut self, out: &mut impl Sink<Effect>) -> bool {
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
    fn restore(&mut self) -> bool {
        self.row = self.saved.row;
        self.col = self.saved.col;
        self.saved.pending
    }
}

#[derive(Debug)]
pub struct Normal(());
#[derive(Debug)]
pub struct Pending(());
#[derive(Debug)]
pub struct NormalNoWrap(());
#[derive(Debug)]
pub struct PendingNoWrap(());

/// Autowrap on, after a print or a restore.
pub enum Ready {
    Normal(Normal),
    Pending(Pending),
}

/// Autowrap off, after a print or a restore.
pub enum ReadyNoWrap {
    Normal(NormalNoWrap),
    Pending(PendingNoWrap),
}

impl Normal {
    pub fn new(config: Config) -> (Normal, Screen) {
        (Normal(()), Screen::new(config))
    }
    #[inline]
    pub fn print(self, s: &mut Screen, wide: bool, out: &mut impl Sink<Effect>) -> Ready {
        s.previous = Some(wide);
        if wide && s.col + 1 >= s.cols {
            out.push(Effect::SpacerHead { row: s.row, col: s.col });
            s.soft_wrap(out);
        }
        if s.put(wide, out) { Ready::Pending(Pending(())) } else { Ready::Normal(self) }
    }
    pub fn clearing(self, s: &mut Screen, op: Clearing, out: &mut impl Sink<Effect>) -> Normal {
        s.clearing(op, out);
        self
    }
    pub fn keeping(self, s: &mut Screen, op: Keeping) -> Normal {
        s.keeping(op, false);
        self
    }
    pub fn reverse_index(self, s: &mut Screen, out: &mut impl Sink<Effect>) -> Normal {
        s.reverse_index(out);
        self
    }
    pub fn restore(self, s: &mut Screen) -> Ready {
        if s.restore() { Ready::Pending(Pending(())) } else { Ready::Normal(self) }
    }
    pub fn autowrap_off(self) -> NormalNoWrap {
        NormalNoWrap(())
    }
}

impl Pending {
    /// Wraps first (soft wrap, perhaps a scroll), then prints.
    #[inline]
    pub fn print(self, s: &mut Screen, wide: bool, out: &mut impl Sink<Effect>) -> Ready {
        s.soft_wrap(out);
        Normal(()).print(s, wide, out)
    }
    pub fn clearing(self, s: &mut Screen, op: Clearing, out: &mut impl Sink<Effect>) -> Normal {
        s.clearing(op, out);
        Normal(())
    }
    pub fn keeping(self, s: &mut Screen, op: Keeping) -> Pending {
        s.keeping(op, true);
        self
    }
    /// At the top it scrolls and keeps the pending wrap; else it moves up and clears it.
    pub fn reverse_index(self, s: &mut Screen, out: &mut impl Sink<Effect>) -> Result<Pending, Normal> {
        if s.reverse_index(out) { Ok(self) } else { Err(Normal(())) }
    }
    pub fn restore(self, s: &mut Screen) -> Ready {
        if s.restore() { Ready::Pending(self) } else { Ready::Normal(Normal(())) }
    }
    pub fn autowrap_off(self) -> PendingNoWrap {
        PendingNoWrap(())
    }
}

impl NormalNoWrap {
    #[inline]
    pub fn print(self, s: &mut Screen, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap {
        s.previous = Some(wide);
        if wide && s.col + 1 >= s.cols {
            return ReadyNoWrap::Normal(self);
        }
        if s.put(wide, out) { ReadyNoWrap::Pending(PendingNoWrap(())) } else { ReadyNoWrap::Normal(self) }
    }
    pub fn clearing(self, s: &mut Screen, op: Clearing, out: &mut impl Sink<Effect>) -> NormalNoWrap {
        s.clearing(op, out);
        self
    }
    pub fn keeping(self, s: &mut Screen, op: Keeping) -> NormalNoWrap {
        s.keeping(op, false);
        self
    }
    pub fn reverse_index(self, s: &mut Screen, out: &mut impl Sink<Effect>) -> NormalNoWrap {
        s.reverse_index(out);
        self
    }
    pub fn restore(self, s: &mut Screen) -> ReadyNoWrap {
        if s.restore() { ReadyNoWrap::Pending(PendingNoWrap(())) } else { ReadyNoWrap::Normal(self) }
    }
    pub fn autowrap_on(self) -> Normal {
        Normal(())
    }
}

impl PendingNoWrap {
    /// No wrap without autowrap: the print overwrites the last column.
    #[inline]
    pub fn print(self, s: &mut Screen, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap {
        NormalNoWrap(()).print(s, wide, out)
    }
    pub fn clearing(self, s: &mut Screen, op: Clearing, out: &mut impl Sink<Effect>) -> NormalNoWrap {
        s.clearing(op, out);
        NormalNoWrap(())
    }
    pub fn keeping(self, s: &mut Screen, op: Keeping) -> PendingNoWrap {
        s.keeping(op, true);
        self
    }
    pub fn reverse_index(self, s: &mut Screen, out: &mut impl Sink<Effect>) -> Result<PendingNoWrap, NormalNoWrap> {
        if s.reverse_index(out) { Ok(self) } else { Err(NormalNoWrap(())) }
    }
    pub fn restore(self, s: &mut Screen) -> ReadyNoWrap {
        if s.restore() { ReadyNoWrap::Pending(self) } else { ReadyNoWrap::Normal(NormalNoWrap(())) }
    }
    pub fn autowrap_on(self) -> Pending {
        Pending(())
    }
}

/// The runtime wrapper: one byte.
#[derive(Debug)]
pub enum Any {
    Normal(Normal),
    Pending(Pending),
    NormalNoWrap(NormalNoWrap),
    PendingNoWrap(PendingNoWrap),
}

impl From<Ready> for Any {
    #[inline]
    fn from(r: Ready) -> Any {
        match r {
            Ready::Normal(s) => Any::Normal(s),
            Ready::Pending(s) => Any::Pending(s),
        }
    }
}

impl From<ReadyNoWrap> for Any {
    #[inline]
    fn from(r: ReadyNoWrap) -> Any {
        match r {
            ReadyNoWrap::Normal(s) => Any::NormalNoWrap(s),
            ReadyNoWrap::Pending(s) => Any::PendingNoWrap(s),
        }
    }
}

impl Any {
    #[inline]
    fn print(self, s: &mut Screen, wide: bool, out: &mut impl Sink<Effect>) -> Any {
        match self {
            Any::Normal(t) => t.print(s, wide, out).into(),
            Any::Pending(t) => t.print(s, wide, out).into(),
            Any::NormalNoWrap(t) => t.print(s, wide, out).into(),
            Any::PendingNoWrap(t) => t.print(s, wide, out).into(),
        }
    }
}

pub struct Wrap {
    config: Config,
    screen: Screen,
    state: Any,
}

impl Wrap {
    pub fn typed(&self) -> &Any {
        &self.state
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        let (state, screen) = Normal::new(config);
        Self { config, screen, state: Any::Normal(state) }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        // A placeholder while the token is moved out (never observed; zero-sized).
        let state = std::mem::replace(&mut self.state, Any::Normal(Normal(())));
        let s = &mut self.screen;
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
        self.state = match (state, event) {
            (t, Event::Print { wide }) => t.print(s, wide, out),
            (t, Event::Repeat(n)) => match s.previous {
                Some(wide) => {
                    let mut t = t;
                    for _ in 0..n.max(1) {
                        t = t.print(s, wide, out);
                    }
                    t
                }
                None => t,
            },
            (_, Event::FullReset) => {
                let (t, screen) = Normal::new(self.config);
                *s = screen;
                Any::Normal(t)
            }
            (Any::Normal(t), Event::SetAutowrap(false)) => Any::NormalNoWrap(t.autowrap_off()),
            (Any::Pending(t), Event::SetAutowrap(false)) => Any::PendingNoWrap(t.autowrap_off()),
            (Any::NormalNoWrap(t), Event::SetAutowrap(true)) => Any::Normal(t.autowrap_on()),
            (Any::PendingNoWrap(t), Event::SetAutowrap(true)) => Any::Pending(t.autowrap_on()),
            (t, Event::SetAutowrap(_)) => t,
            (Any::Normal(t), Event::ReverseIndex) => Any::Normal(t.reverse_index(s, out)),
            (Any::NormalNoWrap(t), Event::ReverseIndex) => Any::NormalNoWrap(t.reverse_index(s, out)),
            (Any::Pending(t), Event::ReverseIndex) => match t.reverse_index(s, out) {
                Ok(p) => Any::Pending(p),
                Err(n) => Any::Normal(n),
            },
            (Any::PendingNoWrap(t), Event::ReverseIndex) => match t.reverse_index(s, out) {
                Ok(p) => Any::PendingNoWrap(p),
                Err(n) => Any::NormalNoWrap(n),
            },
            (Any::Normal(t), Event::RestoreCursor) => t.restore(s).into(),
            (Any::Pending(t), Event::RestoreCursor) => t.restore(s).into(),
            (Any::NormalNoWrap(t), Event::RestoreCursor) => t.restore(s).into(),
            (Any::PendingNoWrap(t), Event::RestoreCursor) => t.restore(s).into(),
            (t, _) => match (t, clearing, keeping) {
                (Any::Normal(t), Some(op), _) => Any::Normal(t.clearing(s, op, out)),
                (Any::Pending(t), Some(op), _) => Any::Normal(t.clearing(s, op, out)),
                (Any::NormalNoWrap(t), Some(op), _) => Any::NormalNoWrap(t.clearing(s, op, out)),
                (Any::PendingNoWrap(t), Some(op), _) => Any::NormalNoWrap(t.clearing(s, op, out)),
                (Any::Normal(t), None, Some(op)) => Any::Normal(t.keeping(s, op)),
                (Any::Pending(t), None, Some(op)) => Any::Pending(t.keeping(s, op)),
                (Any::NormalNoWrap(t), None, Some(op)) => Any::NormalNoWrap(t.keeping(s, op)),
                (Any::PendingNoWrap(t), None, Some(op)) => Any::PendingNoWrap(t.keeping(s, op)),
                (t, None, None) => t,
            },
        };
    }

    fn state(&self) -> StateName {
        match self.state {
            Any::Normal(_) => StateName::Normal,
            Any::Pending(_) => StateName::PendingWrap,
            Any::NormalNoWrap(_) => StateName::NormalNoAutowrap,
            Any::PendingNoWrap(_) => StateName::PendingNoAutowrap,
        }
    }
}
