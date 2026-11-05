//! `wrap` in Hotaru's shape: `Screen.wrap_pending`, `State.autowrap`,
//! `State.previous_char`, `SavedCursor.wrap_pending`, and per-row
//! `Row.wrapped` / `Row.wrap_continuation` flags (two masks here), mutated
//! by each control's own code (screen.rs:1129-1154, 2267-2450).

use spec::Sink;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, Default)]
pub struct SavedCursor {
    pub row: u16,
    pub col: u16,
    pub wrap_pending: bool,
}

/// The machine's fields, in the current code's shape. Readable through
/// the machine (`Deref`); only `handle` changes them.
pub struct Fields {
    pub cols: u16,
    pub rows: u16,
    pub row: u16,
    pub col: u16,
    pub wrap_pending: bool,
    pub autowrap: bool,
    pub saved: SavedCursor,
    pub previous_char: Option<bool>,
    /// `Row.wrapped`, bit per row.
    pub wrapped: u128,
    /// `Row.wrap_continuation`, bit per row.
    pub continuation: u128,
}

impl Fields {
    fn mask(&self) -> u128 {
        if self.rows >= 128 { u128::MAX } else { (1u128 << self.rows) - 1 }
    }

    #[inline]
    fn linefeed(&mut self, out: &mut impl Sink<Effect>) {
        self.wrap_pending = false;
        if self.row == self.rows - 1 {
            out.push(Effect::ScrollUp);
            self.wrapped >>= 1;
            self.continuation >>= 1;
        } else {
            self.row += 1;
        }
    }

    #[cold]
    fn wrap(&mut self, out: &mut impl Sink<Effect>) {
        self.wrap_pending = false;
        if self.autowrap {
            out.push(Effect::SoftWrap { row: self.row });
            self.wrapped |= 1 << self.row;
            self.linefeed(out);
            self.col = 0;
            self.continuation |= 1 << self.row;
        }
    }

    #[inline]
    fn print(&mut self, wide: bool, out: &mut impl Sink<Effect>) {
        self.previous_char = Some(wide);
        if self.wrap_pending {
            self.wrap(out);
        }
        if wide && self.col + 1 >= self.cols {
            if !self.autowrap {
                return;
            }
            out.push(Effect::SpacerHead { row: self.row, col: self.col });
            out.push(Effect::SoftWrap { row: self.row });
            self.wrapped |= 1 << self.row;
            self.col = 0;
            self.linefeed(out);
            self.continuation |= 1 << self.row;
        }
        out.push(Effect::Put { row: self.row, col: self.col, wide });
        self.col += if wide { 2 } else { 1 };
        if self.col >= self.cols {
            self.col = self.cols - 1;
            self.wrap_pending = true;
        }
    }
}

impl Fields {
    fn new(config: Config) -> Self {
        assert!(config.cols >= 2 && config.rows >= 1 && config.rows <= spec::wrap::MAX_ROWS);
        Self {
            cols: config.cols,
            rows: config.rows,
            row: 0,
            col: 0,
            wrap_pending: false,
            autowrap: true,
            saved: SavedCursor::default(),
            previous_char: None,
            wrapped: 0,
            continuation: 0,
        }
    }

    #[inline]
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        match event {
            Event::Print { wide } => self.print(wide, out),
            Event::Repeat(n) => {
                if let Some(wide) = self.previous_char {
                    for _ in 0..n.max(1) {
                        self.print(wide, out);
                    }
                }
            }
            Event::CarriageReturn => {
                self.col = 0;
                self.wrap_pending = false;
            }
            Event::LineFeed => self.linefeed(out),
            Event::Backspace => {
                self.col = self.col.saturating_sub(1);
                self.wrap_pending = false;
            }
            Event::Tab => {
                let end = self.cols - 1;
                self.col = (self.col + 1..=end).find(|c| c % 8 == 0).unwrap_or(end);
            }
            Event::CursorTo { row, col } => {
                self.row = row.min(self.rows - 1);
                self.col = col.min(self.cols - 1);
                self.wrap_pending = false;
            }
            Event::ReverseIndex => {
                if self.row == 0 {
                    out.push(Effect::ScrollDown);
                    // The bottom row is lost; the new top row is reset.
                    self.wrapped = (self.wrapped << 1) & self.mask();
                    self.continuation = (self.continuation << 1) & self.mask();
                } else {
                    self.wrap_pending = false;
                    self.row -= 1;
                }
            }
            Event::EraseLine(m) => {
                if m > 2 {
                    return;
                }
                if matches!(m, 0 | 2) && self.wrapped & (1 << self.row) != 0 {
                    self.wrapped &= !(1 << self.row);
                    if self.row + 1 < self.rows {
                        self.continuation &= !(1 << (self.row + 1));
                    }
                    out.push(Effect::Unlink { row: self.row });
                }
                self.wrap_pending = false;
            }
            Event::SetAutowrap(on) => self.autowrap = on,
            Event::SaveCursor => {
                self.saved = SavedCursor { row: self.row, col: self.col, wrap_pending: self.wrap_pending };
            }
            Event::RestoreCursor => {
                self.row = self.saved.row.min(self.rows - 1);
                self.col = self.saved.col.min(self.cols - 1);
                self.wrap_pending = self.saved.wrap_pending;
            }
            Event::FullReset => *self = Self::new(Config { cols: self.cols, rows: self.rows }),
        }
    }

    fn state(&self) -> StateName {
        match (self.wrap_pending, self.autowrap) {
            (false, true) => StateName::Normal,
            (true, true) => StateName::PendingWrap,
            (false, false) => StateName::NormalNoAutowrap,
            (true, false) => StateName::PendingNoAutowrap,
        }
    }
}

/// The machine: [`Fields`] read through `Deref`, never written from outside.
pub struct Wrap(Fields);

impl std::ops::Deref for Wrap {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        &self.0
    }
}

impl Machine for Wrap {
    fn new(config: Config) -> Self {
        Wrap(Fields::new(config))
    }
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.0.handle(event, out);
    }
    fn state(&self) -> StateName {
        self.0.state()
    }
}
