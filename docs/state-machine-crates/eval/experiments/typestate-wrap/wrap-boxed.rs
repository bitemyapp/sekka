//! `wrap` as hand-written typestate: `Normal`, `Pending`, `NormalNoWrap`,
//! `PendingNoWrap` each own the screen context. Only `Pending` (autowrap
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
pub struct Normal(Box<Screen>);
#[derive(Debug)]
pub struct Pending(Box<Screen>);
#[derive(Debug)]
pub struct NormalNoWrap(Box<Screen>);
#[derive(Debug)]
pub struct PendingNoWrap(Box<Screen>);

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
    pub fn new(config: Config) -> Self {
        Normal(Box::new(Screen::new(config)))
    }
    #[inline]
    pub fn print(mut self, wide: bool, out: &mut impl Sink<Effect>) -> Ready {
        let s = &mut self.0;
        s.previous = Some(wide);
        if wide && s.col + 1 >= s.cols {
            out.push(Effect::SpacerHead { row: s.row, col: s.col });
            s.soft_wrap(out);
        }
        if s.put(wide, out) { Ready::Pending(Pending(self.0)) } else { Ready::Normal(self) }
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> Normal {
        self.0.clearing(op, out);
        self
    }
    pub fn keeping(mut self, op: Keeping) -> Normal {
        self.0.keeping(op, false);
        self
    }
    pub fn reverse_index(mut self, out: &mut impl Sink<Effect>) -> Normal {
        self.0.reverse_index(out);
        self
    }
    pub fn restore(mut self) -> Ready {
        if self.0.restore() { Ready::Pending(Pending(self.0)) } else { Ready::Normal(self) }
    }
    pub fn autowrap_off(self) -> NormalNoWrap {
        NormalNoWrap(self.0)
    }
}

impl Pending {
    /// Wraps first (soft wrap, perhaps a scroll), then prints.
    #[inline]
    pub fn print(mut self, wide: bool, out: &mut impl Sink<Effect>) -> Ready {
        self.0.soft_wrap(out);
        Normal(self.0).print(wide, out)
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> Normal {
        self.0.clearing(op, out);
        Normal(self.0)
    }
    pub fn keeping(mut self, op: Keeping) -> Pending {
        self.0.keeping(op, true);
        self
    }
    /// At the top it scrolls and keeps the pending wrap; else it moves up and clears it.
    pub fn reverse_index(mut self, out: &mut impl Sink<Effect>) -> Result<Pending, Normal> {
        if self.0.reverse_index(out) { Ok(self) } else { Err(Normal(self.0)) }
    }
    pub fn restore(mut self) -> Ready {
        if self.0.restore() { Ready::Pending(self) } else { Ready::Normal(Normal(self.0)) }
    }
    pub fn autowrap_off(self) -> PendingNoWrap {
        PendingNoWrap(self.0)
    }
}

impl NormalNoWrap {
    #[inline]
    pub fn print(mut self, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap {
        let s = &mut self.0;
        s.previous = Some(wide);
        if wide && s.col + 1 >= s.cols {
            return ReadyNoWrap::Normal(self);
        }
        if s.put(wide, out) { ReadyNoWrap::Pending(PendingNoWrap(self.0)) } else { ReadyNoWrap::Normal(self) }
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> NormalNoWrap {
        self.0.clearing(op, out);
        self
    }
    pub fn keeping(mut self, op: Keeping) -> NormalNoWrap {
        self.0.keeping(op, false);
        self
    }
    pub fn reverse_index(mut self, out: &mut impl Sink<Effect>) -> NormalNoWrap {
        self.0.reverse_index(out);
        self
    }
    pub fn restore(mut self) -> ReadyNoWrap {
        if self.0.restore() { ReadyNoWrap::Pending(PendingNoWrap(self.0)) } else { ReadyNoWrap::Normal(self) }
    }
    pub fn autowrap_on(self) -> Normal {
        Normal(self.0)
    }
}

impl PendingNoWrap {
    /// No wrap without autowrap: the print overwrites the last column.
    #[inline]
    pub fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap {
        NormalNoWrap(self.0).print(wide, out)
    }
    pub fn clearing(mut self, op: Clearing, out: &mut impl Sink<Effect>) -> NormalNoWrap {
        self.0.clearing(op, out);
        NormalNoWrap(self.0)
    }
    pub fn keeping(mut self, op: Keeping) -> PendingNoWrap {
        self.0.keeping(op, true);
        self
    }
    pub fn reverse_index(mut self, out: &mut impl Sink<Effect>) -> Result<PendingNoWrap, NormalNoWrap> {
        if self.0.reverse_index(out) { Ok(self) } else { Err(NormalNoWrap(self.0)) }
    }
    pub fn restore(mut self) -> ReadyNoWrap {
        if self.0.restore() { ReadyNoWrap::Pending(self) } else { ReadyNoWrap::Normal(NormalNoWrap(self.0)) }
    }
    pub fn autowrap_on(self) -> Pending {
        Pending(self.0)
    }
}

/// The runtime wrapper.
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
    fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> Any {
        match self {
            Any::Normal(s) => s.print(wide, out).into(),
            Any::Pending(s) => s.print(wide, out).into(),
            Any::NormalNoWrap(s) => s.print(wide, out).into(),
            Any::PendingNoWrap(s) => s.print(wide, out).into(),
        }
    }
    fn screen(&self) -> &Screen {
        match self {
            Any::Normal(s) => &s.0,
            Any::Pending(s) => &s.0,
            Any::NormalNoWrap(s) => &s.0,
            Any::PendingNoWrap(s) => &s.0,
        }
    }
}

pub struct Wrap {
    config: Config,
    state: Option<Any>,
}

impl Wrap {
    pub fn typed(&self) -> &Any {
        self.state.as_ref().expect("a state")
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        Self { config, state: Some(Any::Normal(Normal::new(config))) }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        // `None` only while the state is moved out (never observed).
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
            (s, Event::Print { wide }) => s.print(wide, out),
            (s, Event::Repeat(n)) => match s.screen().previous {
                Some(wide) => {
                    let mut s = s;
                    for _ in 0..n.max(1) {
                        s = s.print(wide, out);
                    }
                    s
                }
                None => s,
            },
            (_, Event::FullReset) => Any::Normal(Normal::new(self.config)),
            (Any::Normal(s), Event::SetAutowrap(false)) => Any::NormalNoWrap(s.autowrap_off()),
            (Any::Pending(s), Event::SetAutowrap(false)) => Any::PendingNoWrap(s.autowrap_off()),
            (Any::NormalNoWrap(s), Event::SetAutowrap(true)) => Any::Normal(s.autowrap_on()),
            (Any::PendingNoWrap(s), Event::SetAutowrap(true)) => Any::Pending(s.autowrap_on()),
            (s, Event::SetAutowrap(_)) => s,
            (Any::Normal(s), Event::ReverseIndex) => Any::Normal(s.reverse_index(out)),
            (Any::NormalNoWrap(s), Event::ReverseIndex) => Any::NormalNoWrap(s.reverse_index(out)),
            (Any::Pending(s), Event::ReverseIndex) => match s.reverse_index(out) {
                Ok(p) => Any::Pending(p),
                Err(n) => Any::Normal(n),
            },
            (Any::PendingNoWrap(s), Event::ReverseIndex) => match s.reverse_index(out) {
                Ok(p) => Any::PendingNoWrap(p),
                Err(n) => Any::NormalNoWrap(n),
            },
            (Any::Normal(s), Event::RestoreCursor) => s.restore().into(),
            (Any::Pending(s), Event::RestoreCursor) => s.restore().into(),
            (Any::NormalNoWrap(s), Event::RestoreCursor) => s.restore().into(),
            (Any::PendingNoWrap(s), Event::RestoreCursor) => s.restore().into(),
            (s, _) => match (s, clearing, keeping) {
                (Any::Normal(s), Some(op), _) => Any::Normal(s.clearing(op, out)),
                (Any::Pending(s), Some(op), _) => Any::Normal(s.clearing(op, out)),
                (Any::NormalNoWrap(s), Some(op), _) => Any::NormalNoWrap(s.clearing(op, out)),
                (Any::PendingNoWrap(s), Some(op), _) => Any::NormalNoWrap(s.clearing(op, out)),
                (Any::Normal(s), None, Some(op)) => Any::Normal(s.keeping(op)),
                (Any::Pending(s), None, Some(op)) => Any::Pending(s.keeping(op)),
                (Any::NormalNoWrap(s), None, Some(op)) => Any::NormalNoWrap(s.keeping(op)),
                (Any::PendingNoWrap(s), None, Some(op)) => Any::PendingNoWrap(s.keeping(op)),
                (s, None, None) => s,
            },
        });
    }

    fn state(&self) -> StateName {
        match self.typed() {
            Any::Normal(_) => StateName::Normal,
            Any::Pending(_) => StateName::PendingWrap,
            Any::NormalNoWrap(_) => StateName::NormalNoAutowrap,
            Any::PendingNoWrap(_) => StateName::PendingNoAutowrap,
        }
    }
}
