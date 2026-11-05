//! `wrap` as hand-written typestate whose states hold the screen by
//! `&mut`: `Normal<'s>`, `Pending<'s>`, … each carry the one borrow of the
//! machine's screen, so a state can't be paired with another screen. The
//! machine stores a one-byte tag and rebuilds the typed state per event. Only `Pending` (autowrap
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
pub struct Normal<'s>(&'s mut Screen);
#[derive(Debug)]
pub struct Pending<'s>(&'s mut Screen);
#[derive(Debug)]
pub struct NormalNoWrap<'s>(&'s mut Screen);
#[derive(Debug)]
pub struct PendingNoWrap<'s>(&'s mut Screen);

/// Autowrap on, after a print or a restore.
pub enum Ready<'s> {
    Normal(Normal<'s>),
    Pending(Pending<'s>),
}

/// Autowrap off, after a print or a restore.
pub enum ReadyNoWrap<'s> {
    Normal(NormalNoWrap<'s>),
    Pending(PendingNoWrap<'s>),
}

impl<'s> Normal<'s> {
    #[inline]
    pub fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> Ready<'s> {
        let s = &mut *self.0;
        s.previous = Some(wide);
        if wide && s.col + 1 >= s.cols {
            out.push(Effect::SpacerHead { row: s.row, col: s.col });
            s.soft_wrap(out);
        }
        if s.put(wide, out) { Ready::Pending(Pending(self.0)) } else { Ready::Normal(self) }
    }
    pub fn clearing(self, op: Clearing, out: &mut impl Sink<Effect>) -> Normal<'s> {
        self.0.clearing(op, out);
        self
    }
    pub fn keeping(self, op: Keeping) -> Normal<'s> {
        self.0.keeping(op, false);
        self
    }
    pub fn reverse_index(self, out: &mut impl Sink<Effect>) -> Normal<'s> {
        self.0.reverse_index(out);
        self
    }
    pub fn restore(self) -> Ready<'s> {
        if self.0.restore() { Ready::Pending(Pending(self.0)) } else { Ready::Normal(self) }
    }
    pub fn autowrap_off(self) -> NormalNoWrap<'s> {
        NormalNoWrap(self.0)
    }
}

impl<'s> Pending<'s> {
    /// Wraps first (soft wrap, perhaps a scroll), then prints.
    #[inline]
    pub fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> Ready<'s> {
        self.0.soft_wrap(out);
        Normal(self.0).print(wide, out)
    }
    pub fn clearing(self, op: Clearing, out: &mut impl Sink<Effect>) -> Normal<'s> {
        self.0.clearing(op, out);
        Normal(self.0)
    }
    pub fn keeping(self, op: Keeping) -> Pending<'s> {
        self.0.keeping(op, true);
        self
    }
    /// At the top it scrolls and keeps the pending wrap; else it moves up and clears it.
    pub fn reverse_index(self, out: &mut impl Sink<Effect>) -> Result<Pending<'s>, Normal<'s>> {
        if self.0.reverse_index(out) { Ok(self) } else { Err(Normal(self.0)) }
    }
    pub fn restore(self) -> Ready<'s> {
        if self.0.restore() { Ready::Pending(self) } else { Ready::Normal(Normal(self.0)) }
    }
    pub fn autowrap_off(self) -> PendingNoWrap<'s> {
        PendingNoWrap(self.0)
    }
}

impl<'s> NormalNoWrap<'s> {
    #[inline]
    pub fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap<'s> {
        let s = &mut *self.0;
        s.previous = Some(wide);
        if wide && s.col + 1 >= s.cols {
            return ReadyNoWrap::Normal(self);
        }
        if s.put(wide, out) { ReadyNoWrap::Pending(PendingNoWrap(self.0)) } else { ReadyNoWrap::Normal(self) }
    }
    pub fn clearing(self, op: Clearing, out: &mut impl Sink<Effect>) -> NormalNoWrap<'s> {
        self.0.clearing(op, out);
        self
    }
    pub fn keeping(self, op: Keeping) -> NormalNoWrap<'s> {
        self.0.keeping(op, false);
        self
    }
    pub fn reverse_index(self, out: &mut impl Sink<Effect>) -> NormalNoWrap<'s> {
        self.0.reverse_index(out);
        self
    }
    pub fn restore(self) -> ReadyNoWrap<'s> {
        if self.0.restore() { ReadyNoWrap::Pending(PendingNoWrap(self.0)) } else { ReadyNoWrap::Normal(self) }
    }
    pub fn autowrap_on(self) -> Normal<'s> {
        Normal(self.0)
    }
}

impl<'s> PendingNoWrap<'s> {
    /// No wrap without autowrap: the print overwrites the last column.
    #[inline]
    pub fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> ReadyNoWrap<'s> {
        NormalNoWrap(self.0).print(wide, out)
    }
    pub fn clearing(self, op: Clearing, out: &mut impl Sink<Effect>) -> NormalNoWrap<'s> {
        self.0.clearing(op, out);
        NormalNoWrap(self.0)
    }
    pub fn keeping(self, op: Keeping) -> PendingNoWrap<'s> {
        self.0.keeping(op, true);
        self
    }
    pub fn reverse_index(self, out: &mut impl Sink<Effect>) -> Result<PendingNoWrap<'s>, NormalNoWrap<'s>> {
        if self.0.reverse_index(out) { Ok(self) } else { Err(NormalNoWrap(self.0)) }
    }
    pub fn restore(self) -> ReadyNoWrap<'s> {
        if self.0.restore() { ReadyNoWrap::Pending(self) } else { ReadyNoWrap::Normal(NormalNoWrap(self.0)) }
    }
    pub fn autowrap_on(self) -> Pending<'s> {
        Pending(self.0)
    }
}

/// Which state the machine is in (one byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Normal,
    Pending,
    NormalNoWrap,
    PendingNoWrap,
}

/// The typed state, holding the machine's screen.
#[derive(Debug)]
pub enum Any<'s> {
    Normal(Normal<'s>),
    Pending(Pending<'s>),
    NormalNoWrap(NormalNoWrap<'s>),
    PendingNoWrap(PendingNoWrap<'s>),
}

impl<'s> From<Ready<'s>> for Any<'s> {
    #[inline]
    fn from(r: Ready<'s>) -> Any<'s> {
        match r {
            Ready::Normal(s) => Any::Normal(s),
            Ready::Pending(s) => Any::Pending(s),
        }
    }
}

impl<'s> From<ReadyNoWrap<'s>> for Any<'s> {
    #[inline]
    fn from(r: ReadyNoWrap<'s>) -> Any<'s> {
        match r {
            ReadyNoWrap::Normal(s) => Any::NormalNoWrap(s),
            ReadyNoWrap::Pending(s) => Any::PendingNoWrap(s),
        }
    }
}

impl<'s> Any<'s> {
    #[inline]
    fn open(tag: Tag, s: &'s mut Screen) -> Any<'s> {
        match tag {
            Tag::Normal => Any::Normal(Normal(s)),
            Tag::Pending => Any::Pending(Pending(s)),
            Tag::NormalNoWrap => Any::NormalNoWrap(NormalNoWrap(s)),
            Tag::PendingNoWrap => Any::PendingNoWrap(PendingNoWrap(s)),
        }
    }
    #[inline]
    fn tag(&self) -> Tag {
        match self {
            Any::Normal(_) => Tag::Normal,
            Any::Pending(_) => Tag::Pending,
            Any::NormalNoWrap(_) => Tag::NormalNoWrap,
            Any::PendingNoWrap(_) => Tag::PendingNoWrap,
        }
    }
    #[inline]
    fn print(self, wide: bool, out: &mut impl Sink<Effect>) -> Any<'s> {
        match self {
            Any::Normal(t) => t.print(wide, out).into(),
            Any::Pending(t) => t.print(wide, out).into(),
            Any::NormalNoWrap(t) => t.print(wide, out).into(),
            Any::PendingNoWrap(t) => t.print(wide, out).into(),
        }
    }
}

pub struct Wrap {
    config: Config,
    screen: Screen,
    tag: Tag,
}

impl Wrap {
    pub fn typed(&self) -> Tag {
        self.tag
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        Self { config, screen: Screen::new(config), tag: Tag::Normal }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        if let Event::FullReset = event {
            self.screen = Screen::new(self.config);
            self.tag = Tag::Normal;
            return;
        }
        let previous = self.screen.previous;
        let state = Any::open(self.tag, &mut self.screen);
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
        let next: Any<'_> = match (state, event) {
            (t, Event::Print { wide }) => t.print(wide, out),
            (t, Event::Repeat(n)) => match previous {
                Some(wide) => {
                    let mut t = t;
                    for _ in 0..n.max(1) {
                        t = t.print(wide, out);
                    }
                    t
                }
                None => t,
            },
            (Any::Normal(t), Event::SetAutowrap(false)) => Any::NormalNoWrap(t.autowrap_off()),
            (Any::Pending(t), Event::SetAutowrap(false)) => Any::PendingNoWrap(t.autowrap_off()),
            (Any::NormalNoWrap(t), Event::SetAutowrap(true)) => Any::Normal(t.autowrap_on()),
            (Any::PendingNoWrap(t), Event::SetAutowrap(true)) => Any::Pending(t.autowrap_on()),
            (t, Event::SetAutowrap(_)) => t,
            (Any::Normal(t), Event::ReverseIndex) => Any::Normal(t.reverse_index(out)),
            (Any::NormalNoWrap(t), Event::ReverseIndex) => Any::NormalNoWrap(t.reverse_index(out)),
            (Any::Pending(t), Event::ReverseIndex) => match t.reverse_index(out) {
                Ok(p) => Any::Pending(p),
                Err(n) => Any::Normal(n),
            },
            (Any::PendingNoWrap(t), Event::ReverseIndex) => match t.reverse_index(out) {
                Ok(p) => Any::PendingNoWrap(p),
                Err(n) => Any::NormalNoWrap(n),
            },
            (Any::Normal(t), Event::RestoreCursor) => t.restore().into(),
            (Any::Pending(t), Event::RestoreCursor) => t.restore().into(),
            (Any::NormalNoWrap(t), Event::RestoreCursor) => t.restore().into(),
            (Any::PendingNoWrap(t), Event::RestoreCursor) => t.restore().into(),
            (t, _) => match (t, clearing, keeping) {
                (Any::Normal(t), Some(op), _) => Any::Normal(t.clearing(op, out)),
                (Any::Pending(t), Some(op), _) => Any::Normal(t.clearing(op, out)),
                (Any::NormalNoWrap(t), Some(op), _) => Any::NormalNoWrap(t.clearing(op, out)),
                (Any::PendingNoWrap(t), Some(op), _) => Any::NormalNoWrap(t.clearing(op, out)),
                (Any::Normal(t), None, Some(op)) => Any::Normal(t.keeping(op)),
                (Any::Pending(t), None, Some(op)) => Any::Pending(t.keeping(op)),
                (Any::NormalNoWrap(t), None, Some(op)) => Any::NormalNoWrap(t.keeping(op)),
                (Any::PendingNoWrap(t), None, Some(op)) => Any::PendingNoWrap(t.keeping(op)),
                (t, None, None) => t,
            },
        };
        self.tag = next.tag();
    }

    fn state(&self) -> StateName {
        match self.tag {
            Tag::Normal => StateName::Normal,
            Tag::Pending => StateName::PendingWrap,
            Tag::NormalNoWrap => StateName::NormalNoAutowrap,
            Tag::PendingNoWrap => StateName::PendingNoAutowrap,
        }
    }
}
