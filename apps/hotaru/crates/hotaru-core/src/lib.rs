//! Linux Hotaru terminal engine. No window system, GTK, Zig runtime or foreign
//! terminal emulator is involved. See docs/upstream.md for the port coverage.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

mod content;
pub mod formatter;
mod grapheme;
mod history;
pub mod hyperlink;
pub mod input;
mod memory;
mod parser;
mod position;
mod row;
#[allow(unsafe_code)]
mod scan;
mod screen;
pub mod search;
mod style;
mod unicode;

pub use content::{ContentCancelled, ContentRetention, ContentRows, ContentSnapshot};
pub use history::{DEFAULT_HISTORY_BYTES, HistoryLimits, HistoryMemory, HistorySnapshot};
pub use position::{ContentEpoch, ContentPosition};
pub use row::{Cell, Row};
pub use screen::{Cursor, Modes, TerminalEvent};
pub use style::{Color, Rgb, Style, Underline, attrs};

/// Terminal width policy. Native applications should select their configured
/// default before consuming PTY input; DEC mode 2027 can change it afterward.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphemeWidthMethod {
    #[default]
    Legacy,
    Unicode,
}

/// Shared immutable row ownership. Rows never need weak references, so this
/// Arc can check uniqueness without modifying a second atomic counter.
pub type RowRef = triomphe::Arc<Row>;

pub const MAX_COLS: usize = 8192;
pub const MAX_ROWS: usize = 4096;
pub(crate) const MAX_GRAPHEME_SCALARS: usize = 65;
pub(crate) const MAX_GRAPHEME_BYTES: usize = MAX_GRAPHEME_SCALARS * 4;

/// Application-configured colors. OSC overrides are tracked separately so a
/// theme change preserves a program's explicit colors, even when RGBs match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalColors {
    pub palette: [Rgb; 256],
    pub foreground: Rgb,
    pub background: Rgb,
}

impl Default for TerminalColors {
    fn default() -> Self {
        Self {
            palette: style::default_palette(),
            foreground: Rgb(229, 229, 229),
            background: Rgb(0, 0, 0),
        }
    }
}

/// Immutable render/copy ownership boundary. Palette, defaults, rows, styles,
/// and graphemes all remain valid after resize, scrolling, or terminal teardown.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub position: ContentPosition,
    pub cols: usize,
    pub rows: usize,
    pub lines: Vec<RowRef>,
    pub cursor: Cursor,
    pub palette: [Rgb; 256],
    pub default_foreground: Rgb,
    pub default_background: Rgb,
    pub modes: Modes,
    pub generation: u64,
}

impl Snapshot {
    /// Formatting may be large. Call this on a worker, never while holding a
    /// renderer lock or waiting synchronously on the Wayland event-loop thread.
    pub fn plain_text(&self) -> String {
        self.selection_text((0, 0), (self.rows.saturating_sub(1), self.cols))
    }
    /// Row/column endpoints; end is exclusive, reversed selections normalize.
    /// Soft wraps do not introduce newlines. Hard line endings trim blank cells.
    pub fn selection_text(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let mut text = String::new();
        for row in start.0..=end.0.min(self.lines.len().saturating_sub(1)) {
            let line = &self.lines[row];
            let from = if row == start.0 {
                start.1.min(self.cols)
            } else {
                0
            };
            let to = if row == end.0 {
                end.1.min(self.cols)
            } else {
                self.cols
            };
            line.append_text(&mut text, from, to, !line.wrapped());
            if row != end.0 && !line.wrapped() {
                text.push('\n');
            }
        }
        text
    }
}

pub struct Terminal {
    parser: parser::Parser,
    state: screen::State,
}

impl Terminal {
    /// Retain at most `scrollback` physical history rows within the default
    /// 50,000,000-byte logical budget, including active content. Active rows
    /// always fit, even when their metadata exceeds that configured budget.
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        Self::with_history_limits(cols, rows, HistoryLimits::new(scrollback))
    }
    /// Explicit byte and row policies. `bytes: None` deliberately removes the
    /// byte limit; benchmark callers must opt in when measuring larger history.
    pub fn with_history_limits(cols: usize, rows: usize, limits: HistoryLimits) -> Self {
        Self {
            parser: parser::Parser::default(),
            state: screen::State::new(cols, rows, limits),
        }
    }
    /// Consume the whole input. Callers must drain replies between bounded input
    /// chunks. Interactive transports should use `feed_bounded` for backpressure.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.feed_inner(bytes, usize::MAX);
    }
    /// Stop before the next byte once pending replies reach `reply_budget`.
    /// One atomic reply can cross the threshold. Returns bytes consumed; retain
    /// the remainder and drain replies before resuming. No replies are dropped.
    pub fn feed_bounded(&mut self, bytes: &[u8], reply_budget: usize) -> usize {
        self.feed_inner(bytes, reply_budget)
    }
    pub fn pending_response_bytes(&self) -> usize {
        self.state.responses.len()
    }
    pub fn keyboard_options(&self) -> input::KeyboardOptions {
        self.state.keyboard_options()
    }
    pub fn mouse_modes(&self) -> (input::MouseTracking, input::MouseFormat) {
        self.state.mouse_modes()
    }
    pub fn modes(&self) -> Modes {
        self.state.modes()
    }
    /// Set both current mode 2027 and its remembered RIS default. DECRST 2027
    /// still disables clustering; DECSTR leaves the current choice unchanged.
    pub fn set_default_grapheme_width(&mut self, method: GraphemeWidthMethod) {
        self.state.set_default_grapheme_width(method);
    }
    fn feed_inner(&mut self, mut bytes: &[u8], reply_budget: usize) -> usize {
        let original = bytes.len();
        // A failed UTF-8 sequence may leave a long printable byte suffix.
        // Remember its control-free extent so retries do not rescan that
        // suffix for each malformed lead. Consuming bytes only shortens it.
        let mut printable_utf8 = 0usize;
        while !bytes.is_empty() {
            if self.state.responses.len() >= reply_budget {
                break;
            }
            if self.parser.ground() {
                let ascii = scan::printable_ascii(bytes);
                if ascii != 0 {
                    self.state.print_ascii(&bytes[..ascii]);
                    bytes = &bytes[ascii..];
                    printable_utf8 = printable_utf8.saturating_sub(ascii);
                    continue;
                }
                if bytes[0] >= 0x80 {
                    if printable_utf8 == 0 {
                        printable_utf8 = scan::printable_utf8_bytes(bytes);
                    }
                    let end = printable_utf8;
                    let valid = match std::str::from_utf8(&bytes[..end]) {
                        Ok(text) => Some(text),
                        Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()])
                            .ok()
                            .filter(|s| !s.is_empty()),
                    };
                    if let Some(text) = valid {
                        self.state.print_utf8(text);
                        bytes = &bytes[text.len()..];
                        printable_utf8 -= text.len();
                        continue;
                    }
                }
            }
            self.parser.advance(bytes[0], &mut self.state);
            self.state.enforce_history_budget();
            bytes = &bytes[1..];
            printable_utf8 = printable_utf8.saturating_sub(1);
        }
        self.state.generation = self.state.generation.wrapping_add(1);
        self.state.enforce_history_budget();
        original - bytes.len()
    }
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.state.resize(cols, rows);
    }
    /// Replace configured defaults while preserving explicit OSC overrides.
    /// Queries and snapshots share the resulting colors; OSC reset commands
    /// restore these configured values instead of hard-coded terminal defaults.
    pub fn set_default_colors(&mut self, colors: TerminalColors) {
        self.state.set_default_colors(colors);
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.snapshot(0)
    }
    /// Metadata-only viewport anchoring; never restores compressed history.
    pub fn content_position(&self, offset: usize) -> ContentPosition {
        self.state.content_position(offset)
    }
    pub fn generation(&self) -> u64 {
        self.state.generation
    }
    pub fn snapshot_scrolled(&self, offset: usize) -> Snapshot {
        self.state.snapshot(offset)
    }
    pub fn snapshot_all(&self) -> Snapshot {
        self.state.snapshot_all()
    }
    /// Cheap immutable capture for copy/search workers. Capturing does not
    /// decompress history; row iteration on the consuming worker does.
    pub fn content_snapshot(&self) -> ContentSnapshot {
        self.state.content_snapshot()
    }
    pub fn scrollback_len(&self) -> usize {
        self.state.scrollback_len()
    }
    /// Compress one bounded block of cold history. Call only on the terminal
    /// worker during idle time; true means another bounded step may be useful.
    pub fn compress_history_step(&mut self) -> bool {
        self.state.compress_history_step()
    }
    /// History storage accounting excludes snapshots and allocator overhead;
    /// these counts are deliberately distinct from operating-system RSS.
    pub fn history_memory(&self) -> HistoryMemory {
        self.state.history_memory()
    }
    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.state.responses)
    }
    pub fn take_events(&mut self) -> Vec<TerminalEvent> {
        std::mem::take(&mut self.state.events)
    }
}
