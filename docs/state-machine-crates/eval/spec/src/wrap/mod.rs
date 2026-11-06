//! `wrap`: Hotaru's pending wrap (DEC last-column flag), autowrap mode and
//! soft-wrap row linkage, evaluated per printed cell (catalog HCO-12). See
//! `SPEC.md`.

pub mod workload;

#[cfg(feature = "testing")]
pub mod env;
#[cfg(feature = "testing")]
pub mod traces;

use crate::Sink;

/// Screen size. `cols >= 2`, `1 <= rows <= MAX_ROWS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Config {
    pub cols: u16,
    pub rows: u16,
}

/// Rows the link mask can hold (one `u128`).
pub const MAX_ROWS: u16 = 128;

impl Default for Config {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// A printable scalar of width 1 or (`wide`) 2. Width-0 scalars and
    /// grapheme joining are outside this machine.
    Print { wide: bool },
    /// REP (`CSI n b`): the last printed scalar again, `n` times (0 means 1).
    Repeat(u16),
    /// CR.
    CarriageReturn,
    /// LF, VT, FF (and IND): down a row, scrolling at the bottom.
    LineFeed,
    /// BS.
    Backspace,
    /// HT, to the next stop (every 8 columns) or the last column.
    Tab,
    /// CUP and the like, 0-based, clamped to the screen.
    CursorTo { row: u16, col: u16 },
    /// RI (`ESC M`): up a row, scrolling down at the top.
    ReverseIndex,
    /// EL (`CSI n K`): 0 to the end, 1 from the start, 2 the whole line;
    /// other modes are ignored.
    EraseLine(u8),
    /// DECAWM (`CSI ? 7 h/l`).
    SetAutowrap(bool),
    /// DECSC (`ESC 7`).
    SaveCursor,
    /// DECRC (`ESC 8`).
    RestoreCursor,
    /// RIS (`ESC c`).
    FullReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Write the scalar's cell(s) at (row, col); a wide one takes col and col+1.
    Put { row: u16, col: u16, wide: bool },
    /// A wide scalar didn't fit: the last column holds a spacer head.
    SpacerHead { row: u16, col: u16 },
    /// `row` is soft-wrapped: it continues on the next row (which, after
    /// any `ScrollUp` that follows, is marked as its continuation).
    SoftWrap { row: u16 },
    /// The screen scrolled up a row (the top row goes to history).
    ScrollUp,
    /// The screen scrolled down a row (the bottom row is lost).
    ScrollDown,
    /// EL broke `row`'s link to the next row.
    Unlink { row: u16 },
}

/// Pending wrap crossed with autowrap mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateName {
    /// Autowrap on, no wrap pending.
    Normal,
    /// Autowrap on; the last column was printed: the next print wraps first.
    PendingWrap,
    /// Autowrap off, no wrap pending.
    NormalNoAutowrap,
    /// Autowrap off; the last column was printed: the next print overwrites it.
    PendingNoAutowrap,
}

/// An implementation of the wrap machine.
pub trait Machine: Sized {
    fn new(config: Config) -> Self;
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>);
    fn state(&self) -> StateName;
}
