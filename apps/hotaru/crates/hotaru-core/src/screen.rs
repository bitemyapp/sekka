use crate::RowPrompt;
use crate::history::{History, HistoryLimits};
use crate::hyperlink::{Hyperlink, LinkIdentity, LinkRef};
use crate::input::{KeyboardOptions, KittyFlags, KittyKeyboard, MouseFormat, MouseTracking};
use crate::parser::{Csi, Handler, Param};
use crate::style::Underline;
use crate::{
    Cell, Color, MAX_COLS, MAX_ROWS, Rgb, Row, SemanticContent, Snapshot, Style, TerminalColors,
    attrs,
};
use std::collections::VecDeque;
use triomphe::Arc;

/// The shape requested by DECSCUSR, independent of focus and blink phase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub visible: bool,
    pub in_viewport: bool,
    pub shape: CursorShape,
    pub blinking: bool,
}
impl Default for Cursor {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            visible: true,
            in_viewport: true,
            shape: CursorShape::Block,
            blinking: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub focus_events: bool,
    pub alternate_screen: bool,
    pub application_keypad: bool,
    pub backarrow: bool,
    pub ignore_keypad_with_numlock: bool,
    pub alt_esc_prefix: bool,
    pub modify_other_keys: bool,
    pub kitty_flags: KittyFlags,
    pub mouse_tracking: MouseTracking,
    pub mouse_format: MouseFormat,
    pub mouse_alternate_scroll: bool,
    pub grapheme_cluster: bool,
    /// Mode 5522: a paste becomes a Kitty clipboard protocol paste event,
    /// which lists the clipboard's types for the program to read itself.
    pub kitty_paste_events: bool,
}
impl Default for Modes {
    fn default() -> Self {
        Self {
            application_cursor: false,
            bracketed_paste: false,
            focus_events: false,
            alternate_screen: false,
            application_keypad: false,
            backarrow: false,
            ignore_keypad_with_numlock: true,
            alt_esc_prefix: true,
            modify_other_keys: false,
            kitty_flags: KittyFlags(0),
            mouse_tracking: MouseTracking::None,
            mouse_format: MouseFormat::X10,
            mouse_alternate_scroll: true,
            grapheme_cluster: false,
            kitty_paste_events: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    Title(String),
    /// Bounded OSC 7 URI; only the application may decide whether it names a
    /// local working directory. Never execute or trust it as a shell command.
    WorkingDirectory(String),
    Bell,
    /// Raw OSC 52 bytes, not an instruction to accept clipboard access.
    /// The application applies its policy before decoding or requesting data.
    Clipboard {
        selection: u8,
        data: Vec<u8>,
    },
    /// A desktop notification a program asks for: OSC 9 carries only its
    /// body, OSC 777 `notify` a title and body. The application decides
    /// whether and when to show it.
    Notification {
        title: String,
        body: String,
    },
    /// A Kitty clipboard protocol (OSC 5522) read, or a committed write.
    /// Malformed and oversized writes were answered already; the
    /// application applies its policy and answers these.
    KittyClipboard(crate::kitty_clipboard::Request),
}

#[derive(Clone, Copy, Default)]
struct SavedCursor {
    cursor: Cursor,
    style: Style,
    origin: bool,
    wrap_pending: bool,
    charset: [u8; 4],
    active_charset: usize,
    protected: bool,
}

// XTSAVE/XTRESTORE have one saved slot per supported DEC mode, not a stack.
// Unsaved slots use protocol defaults, including when the application's
// configured default for mode 2027 differs. Keep unknown modes unrecognized.
const SAVED_MODE_NUMBERS: &[u16] = &[
    1, 6, 7, 9, 12, 25, 47, 66, 67, 69, 1000, 1002, 1003, 1004, 1005, 1006, 1007, 1015, 1016, 1035,
    1036, 1047, 1048, 1049, 2004, 2027, 5522,
];

// Clipboard requests keep their order, up to a bounded prefix per feed:
// callers drain events after each bounded feed, as the PTY worker does.
// Limits apply before copying; other UI events retain their own slots.
const MAX_CLIPBOARD_EVENTS: usize = 64;
const MAX_CLIPBOARD_BYTES: usize = 8 * 1024 * 1024;

struct SavedModes(u32);

impl Default for SavedModes {
    fn default() -> Self {
        let mut saved = Self(0);
        for mode in [7, 25, 1007, 1035, 1036] {
            saved.set(mode, true);
        }
        saved
    }
}

impl SavedModes {
    fn index(mode: u16) -> Option<usize> {
        SAVED_MODE_NUMBERS.iter().position(|&value| value == mode)
    }
    fn set(&mut self, mode: u16, value: bool) {
        if let Some(index) = Self::index(mode) {
            self.0 = (self.0 & !(1 << index)) | (u32::from(value) << index);
        }
    }
    fn get(&self, mode: u16) -> Option<bool> {
        Self::index(mode).map(|index| self.0 & (1 << index) != 0)
    }
}

struct Screen {
    lines: VecDeque<Arc<Row>>,
    history: History,
    max_history: usize,
    byte_limit: Option<usize>,
    history_budget: Option<usize>,
    budget_signal: Arc<crate::memory::BudgetSignal>,
    cols: usize,
    cursor: Cursor,
    saved: SavedCursor,
    saved_valid: bool,
    wrap_pending: bool,
    top: usize,
    bottom: usize,
    left: usize,
    right: usize,
    kitty_keyboard: KittyKeyboard,
    active_link: Option<LinkRef>,
    // The old destination pen is used to erase on 1049 entry, before the
    // source cursor/pen is copied. State::style owns the active pen.
    inactive_style: Style,
    semantic: SemanticContent,
    semantic_clear_eol: bool,
}

impl Screen {
    fn new(cols: usize, rows: usize, max_history: usize, byte_limit: Option<usize>) -> Self {
        let mut screen = Self {
            lines: (0..rows).map(|_| Arc::new(Row::new(cols))).collect(),
            history: History::default(),
            max_history,
            byte_limit,
            history_budget: byte_limit,
            budget_signal: Arc::new(crate::memory::BudgetSignal::default()),
            cols,
            cursor: Cursor::default(),
            saved: SavedCursor::default(),
            saved_valid: false,
            wrap_pending: false,
            top: 0,
            bottom: rows - 1,
            left: 0,
            right: cols - 1,
            kitty_keyboard: KittyKeyboard::default(),
            active_link: None,
            inactive_style: Style::default(),
            semantic: SemanticContent::Output,
            semantic_clear_eol: false,
        };
        screen.rebase_budget();
        screen
    }
    fn measure_active_bytes(&self) -> usize {
        crate::memory::vector::<Arc<Row>>(self.lines.capacity())
            + self
                .lines
                .iter()
                .map(|row| row.owned_bytes())
                .sum::<usize>()
            + self
                .active_link
                .as_ref()
                .map_or(0, |link| link.owned_bytes())
    }
    fn active_bytes(&self) -> usize {
        self.budget_signal.current()
    }
    fn rebase_budget(&mut self) {
        for row in &mut self.lines {
            if !row.budget_attached(&self.budget_signal) {
                Arc::make_mut(row).attach_budget(&self.budget_signal);
            }
        }
        self.budget_signal.rebase(self.measure_active_bytes());
        self.enforce_budget();
    }
    fn set_active_link(&mut self, link: Option<LinkRef>) {
        let old = self
            .active_link
            .as_ref()
            .map_or(0, |link| link.owned_bytes());
        let new = link.as_ref().map_or(0, |link| link.owned_bytes());
        self.active_link = link;
        self.budget_signal.adjust(old, new);
    }
    fn enforce_budget(&mut self) {
        let Some((current, peak)) = self.budget_signal.take_change() else {
            return;
        };
        self.history.prune(
            self.max_history,
            self.byte_limit.map(|limit| limit.saturating_sub(peak)),
        );
        self.history_budget = self.byte_limit.map(|limit| limit.saturating_sub(current));
    }
    fn row(&mut self) -> &mut Row {
        Arc::make_mut(&mut self.lines[self.cursor.row])
    }
    fn clear_previous_spacer(&mut self) {
        if self.cursor.row > 0 {
            Arc::make_mut(&mut self.lines[self.cursor.row - 1]).clear_spacer_head();
        }
    }
    fn scroll_up(&mut self, count: usize, history: bool, style: Style) {
        if !self.full_width() {
            self.scroll_rectangle(count, false, style);
            return;
        }
        for _ in 0..count.min(self.bottom - self.top + 1) {
            self.enforce_budget();
            let starts_at_top = self.top == 0;
            let old = self.lines.remove(self.top).expect("valid scroll region");
            let old_charge = old.owned_bytes();
            let recycled = if history
                && starts_at_top
                && self.max_history > 0
                && self.history_budget != Some(0)
            {
                self.history.push_back(old);
                if self
                    .history
                    .over_limit(self.max_history, self.history_budget)
                {
                    let row = self.history.pop_front_recycle(self.cols);
                    self.history.prune(self.max_history, self.history_budget);
                    row
                } else {
                    None
                }
            } else {
                Some(old)
            };
            let mut blank = recycled.unwrap_or_else(|| Arc::new(Row::new(self.cols)));
            self.budget_signal.pause();
            Arc::make_mut(&mut blank).reset(self.cols, style);
            if !blank.budget_attached(&self.budget_signal) {
                Arc::make_mut(&mut blank).attach_budget(&self.budget_signal);
            }
            self.budget_signal.resume();
            self.budget_signal.adjust(old_charge, blank.owned_bytes());
            self.lines.insert(self.bottom, blank);
            self.enforce_budget();
        }
    }
    fn scroll_down(&mut self, count: usize, style: Style) {
        if !self.full_width() {
            self.scroll_rectangle(count, true, style);
            return;
        }
        for _ in 0..count.min(self.bottom - self.top + 1) {
            let mut line = self.lines.remove(self.bottom).expect("valid scroll region");
            Arc::make_mut(&mut line).reset(self.cols, style);
            self.lines.insert(self.top, line);
        }
    }
    fn linefeed(&mut self, style: Style) {
        self.linefeed_in_columns(style, self.inside_columns());
    }
    fn linefeed_in_columns(&mut self, style: Style, inside_columns: bool) {
        self.wrap_pending = false;
        if self.cursor.row == self.bottom && inside_columns {
            self.scroll_up(1, true, style);
        } else if self.cursor.row != self.bottom {
            self.cursor.row = (self.cursor.row + 1).min(self.lines.len() - 1);
        }
        if self.semantic_clear_eol {
            self.semantic = SemanticContent::Output;
            self.semantic_clear_eol = false;
        } else if self.semantic != SemanticContent::Output {
            self.row().prompt = RowPrompt::Continuation;
        }
    }
    fn soft_linefeed(&mut self, style: Style) {
        // OSC 133 I ends on an explicit index, never on an automatic wrap.
        let semantic = self.semantic;
        let clear = self.semantic_clear_eol;
        self.linefeed(style);
        self.semantic = semantic;
        self.semantic_clear_eol = clear;
        if semantic == SemanticContent::Prompt {
            self.row().prompt = RowPrompt::Continuation;
        }
    }
    fn full_width(&self) -> bool {
        self.left == 0 && self.right + 1 == self.cols
    }
    fn inside_columns(&self) -> bool {
        (self.left..=self.right).contains(&self.cursor.col)
    }
    fn right_limit(&self) -> usize {
        if self.cursor.col > self.right {
            self.cols
        } else {
            self.right + 1
        }
    }
    fn carriage_return(&mut self, origin: bool) {
        self.cursor.col = if origin || self.cursor.col >= self.left {
            self.left
        } else {
            0
        };
        self.wrap_pending = false;
    }
    fn scroll_rectangle(&mut self, count: usize, down: bool, style: Style) {
        let count = count.min(self.bottom - self.top + 1);
        if count == 0 {
            return;
        }
        let (left, right) = (self.left, self.right + 1);
        for r in self.top..=self.bottom {
            Arc::make_mut(&mut self.lines[r]).prepare_margin_shift(left, right);
        }
        if down {
            for dest in (self.top + count..=self.bottom).rev() {
                let source = self.lines[dest - count].clone();
                Arc::make_mut(&mut self.lines[dest]).copy_margin_from(&source, left, right, style);
                self.enforce_budget();
            }
            for r in self.top..self.top + count {
                Arc::make_mut(&mut self.lines[r]).erase(left, right, style);
            }
        } else {
            for dest in self.top..self.bottom + 1 - count {
                let source = self.lines[dest + count].clone();
                Arc::make_mut(&mut self.lines[dest]).copy_margin_from(&source, left, right, style);
                self.enforce_budget();
            }
            for r in self.bottom + 1 - count..=self.bottom {
                Arc::make_mut(&mut self.lines[r]).erase(left, right, style);
            }
        }
        self.enforce_budget();
    }
    fn resize(&mut self, cols: usize, rows: usize, reflow: bool) {
        self.budget_signal.pause();
        let old_rows = self.lines.len();
        let history = self.history.len();
        let mut buffer = ResizeRows {
            lines: std::mem::take(&mut self.history).into_rows(),
            start: 0,
            cols: self.cols,
            rows: old_rows,
            cursor: (history + self.cursor.row, self.cursor.col),
            saved: self
                .saved_valid
                .then_some((history + self.saved.cursor.row, self.saved.cursor.col)),
            // Keep enough rows for either height until both phases complete.
            limit: self.max_history.saturating_add(old_rows.max(rows)),
        };
        buffer.lines.append(&mut self.lines);
        // Ghostty changes height before narrowing and after widening. Reversing
        // those operations changes which rows are active and where pins land.
        if reflow && cols != self.cols {
            if cols < self.cols {
                buffer.resize_rows(rows, self.cursor.row);
                buffer.reflow(cols, self.cursor.row);
            } else {
                buffer.reflow(cols, self.cursor.row);
                buffer.resize_rows(rows, self.cursor.row);
            }
        } else {
            if cols != self.cols {
                for line in &mut buffer.lines {
                    Arc::make_mut(line).resize(cols);
                }
                buffer.cols = cols;
                buffer.cursor.1 = buffer.cursor.1.min(cols - 1);
                if let Some(saved) = &mut buffer.saved {
                    saved.1 = saved.1.min(cols - 1);
                }
            }
            buffer.resize_rows(rows, self.cursor.row);
        }
        let active = buffer.start + buffer.lines.len() - rows;
        let remap = |pin: (usize, usize)| {
            if pin.0 >= active && pin.0 < active + rows {
                (pin.0 - active, pin.1)
            } else {
                (0, 0)
            }
        };
        (self.cursor.row, self.cursor.col) = remap(buffer.cursor);
        if let Some(saved) = buffer.saved {
            (self.saved.cursor.row, self.saved.cursor.col) = remap(saved);
            if saved.0 < active {
                self.saved.wrap_pending = false;
            }
            if self.saved.wrap_pending && self.saved.cursor.col != cols - 1 {
                self.saved.wrap_pending = false;
                self.saved.cursor.col += 1;
            }
        }
        if self.wrap_pending && self.cursor.col != cols - 1 {
            self.wrap_pending = false;
            self.cursor.col += 1;
        }
        self.lines = buffer.lines.split_off(buffer.lines.len() - rows);
        self.history = History::from_rows(buffer.lines);
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }
        self.cols = cols;
        self.top = 0;
        self.bottom = rows - 1;
        self.left = 0;
        self.right = cols - 1;
        self.rebase_budget();
    }
}

/// A temporary owned row stream with absolute cursor pins. Prefix pruning keeps
/// a hostile narrow resize bounded without invalidating the pin coordinates.
struct ResizeRows {
    lines: VecDeque<Arc<Row>>,
    start: usize,
    cols: usize,
    rows: usize,
    cursor: (usize, usize),
    saved: Option<(usize, usize)>,
    limit: usize,
}

impl ResizeRows {
    fn push_blank(&mut self, continuation: bool) {
        let mut row = Row::new(self.cols);
        row.wrap_continuation = continuation;
        self.lines.push_back(Arc::new(row));
        while self.lines.len() > self.limit {
            self.lines.pop_front();
            self.start += 1;
        }
    }

    fn resize_rows(&mut self, rows: usize, original_cursor_y: usize) {
        if rows < self.rows {
            for _ in rows..self.rows {
                let last = self.start + self.lines.len() - 1;
                if self.lines.back().unwrap().has_text()
                    || self.cursor.0 == last
                    || self.saved.is_some_and(|pin| pin.0 == last)
                {
                    break;
                }
                self.lines.pop_back();
            }
        } else if rows > self.rows && original_cursor_y < self.rows - 1 {
            for _ in self.rows..rows {
                self.push_blank(false);
            }
        }
        self.rows = rows;
        while self.lines.len() < rows {
            self.push_blank(false);
        }
    }

    fn wrapped_above_cursor(&self) -> usize {
        let active = self.start + self.lines.len() - self.rows;
        if self.cursor.0 < active {
            return 0;
        }
        self.lines
            .iter()
            .skip(active - self.start)
            .take(self.cursor.0 - active + 1)
            .filter(|row| row.wrap_continuation)
            .count()
    }

    fn reflow(&mut self, cols: usize, original_cursor_y: usize) {
        let remaining = self.rows.saturating_sub(original_cursor_y + 1);
        let old_wrapped = self.wrapped_above_cursor();
        let source = std::mem::take(&mut self.lines);
        let source_start = self.start;
        let cursor = self.cursor;
        let mut saved = self.saved;
        self.cols = cols;
        self.start = 0;
        self.push_blank(false);
        let mut col = 0;
        let mut pending = false;
        let mut new_rows = 0;
        let mut cursor_mapped = false;
        let mut saved_mapped = false;
        for (source_row, row) in source.into_iter().enumerate() {
            let source_row = source_start + source_row;
            let mut len = row.reflow_len();
            if let Some(pin) = &mut saved
                && pin.0 == source_row
            {
                let dest_col = if new_rows > 0 || pending { 0 } else { col };
                if pin.1 >= len {
                    pin.1 = pin.1.min(cols - 1 - dest_col);
                }
                len = len.max(pin.1 + 1);
            }
            if cursor.0 == source_row {
                len = len.max(cursor.1 + 1);
            }
            if len == 0 {
                if !row.wrap_continuation {
                    new_rows += 1;
                }
                continue;
            }
            for _ in 0..new_rows {
                self.push_blank(false);
            }
            if new_rows > 0 {
                col = 0;
                pending = false;
                new_rows = 0;
            }
            Arc::make_mut(self.lines.back_mut().unwrap()).prompt = row.prompt;
            let mut source_col = 0;
            while source_col < len {
                if pending {
                    Arc::make_mut(self.lines.back_mut().unwrap()).wrapped = true;
                    self.push_blank(true);
                    Arc::make_mut(self.lines.back_mut().unwrap()).prompt = row.prompt;
                    col = 0;
                    pending = false;
                }
                let dest_row = self.start + self.lines.len() - 1;
                if !cursor_mapped && cursor == (source_row, source_col) {
                    self.cursor = (dest_row, col);
                    cursor_mapped = true;
                }
                if !saved_mapped && saved == Some((source_row, source_col)) {
                    self.saved = Some((dest_row, col));
                    saved_mapped = true;
                }
                let cell = row.cells[source_col];
                if cell.is_spacer_head() {
                    source_col += 1;
                    continue;
                }
                if cell.width() == 2 && cols > 1 && col == cols - 1 {
                    let target = Arc::make_mut(self.lines.back_mut().unwrap());
                    target.set_spacer_head(col, Style::default());
                    target.cells[col] = target.cells[col].semantic(cell.semantic_content());
                    target.set_hyperlink_range(col..col + 1, row.hyperlink_ref(source_col));
                    pending = true;
                    continue;
                }
                if cell.width() == 2 && cols == 1 {
                    // The one-column case destroys a wide glyph as Ghostty does.
                    // The skipped spacer pin maps to the same pending-wrap cell.
                    if cursor == (source_row, source_col + 1) {
                        self.cursor = (dest_row, col);
                    }
                    if saved == Some((source_row, source_col + 1)) {
                        self.saved = Some((dest_row, col));
                    }
                    source_col += 2;
                } else {
                    Arc::make_mut(self.lines.back_mut().unwrap()).copy_cell(col, &row, source_col);
                    source_col += 1;
                }
                col += 1;
                if col == cols {
                    col -= 1;
                    pending = true;
                }
            }
            if !row.wrapped {
                new_rows += 1;
            }
        }
        while self.lines.len() < self.rows {
            self.push_blank(false);
        }
        let active = self.start + self.lines.len() - self.rows;
        if self.cursor.0 >= active {
            let wrapped = self.wrapped_above_cursor();
            let current = self.rows.saturating_sub(self.cursor.0 - active + 1);
            let required = remaining
                .saturating_sub(wrapped.saturating_sub(old_wrapped))
                .saturating_sub(current);
            for _ in 0..required {
                self.push_blank(false);
            }
        }
    }
}

pub(crate) struct State {
    primary: Screen,
    alternate: Screen,
    modes: Modes,
    default_grapheme_cluster: bool,
    saved_modes: SavedModes,
    alternate_mode_bits: u8,
    save_cursor_mode: bool,
    previous_char: Option<char>,
    mouse_tracking_modes: u8,
    mouse_format_modes: u8,
    style: Style,
    origin: bool,
    left_right_margin_mode: bool,
    autowrap: bool,
    insert: bool,
    newline: bool,
    charset: [u8; 4],
    active_charset: usize,
    single_shift: Option<usize>,
    protected: bool,
    cursor_blinking: bool,
    default_cursor_shape: CursorShape,
    default_cursor_blinking: bool,
    cursor_is_default: bool,
    next_link_id: u64,
    // Conservative terminal-wide guard: either screen or retained rows may
    // carry prompt metadata after the cursor returns to ordinary output.
    // Only a hard reset can prove that all such state is gone.
    semantic_markers_seen: bool,
    tabs: Vec<bool>,
    default_colors: TerminalColors,
    palette: [Rgb; 256],
    palette_overrides: [bool; 256],
    foreground: Rgb,
    background: Rgb,
    foreground_overridden: bool,
    background_overridden: bool,
    kitty_clipboard: crate::kitty_clipboard::Protocol,
    pub generation: u64,
    pub responses: Vec<u8>,
    pub events: Vec<TerminalEvent>,
}

impl State {
    pub fn new(cols: usize, rows: usize, limits: HistoryLimits) -> Self {
        let cols = cols.clamp(1, MAX_COLS);
        let rows = rows.clamp(1, MAX_ROWS);
        Self {
            primary: Screen::new(cols, rows, limits.rows, limits.bytes),
            alternate: Screen::new(cols, rows, 0, Some(0)),
            modes: Modes::default(),
            default_grapheme_cluster: false,
            saved_modes: SavedModes::default(),
            alternate_mode_bits: 0,
            save_cursor_mode: false,
            previous_char: None,
            mouse_tracking_modes: 0,
            mouse_format_modes: 0,
            style: Style::default(),
            origin: false,
            left_right_margin_mode: false,
            autowrap: true,
            insert: false,
            newline: false,
            charset: [0; 4],
            active_charset: 0,
            single_shift: None,
            protected: false,
            cursor_blinking: false,
            default_cursor_shape: CursorShape::Block,
            default_cursor_blinking: false,
            cursor_is_default: true,
            next_link_id: 0,
            semantic_markers_seen: false,
            tabs: default_tabs(cols),
            default_colors: TerminalColors::default(),
            palette: crate::style::default_palette(),
            palette_overrides: [false; 256],
            foreground: Rgb(229, 229, 229),
            background: Rgb(0, 0, 0),
            foreground_overridden: false,
            background_overridden: false,
            kitty_clipboard: Default::default(),
            generation: 0,
            responses: Vec::new(),
            events: Vec::new(),
        }
    }
    /// RIS: everything but the configured defaults and OSC 10/11 overrides.
    pub fn full_reset(&mut self) {
        let cols = self.screen().cols;
        let rows = self.screen().lines.len();
        let limits = HistoryLimits {
            rows: self.primary.max_history,
            bytes: self.primary.byte_limit,
        };
        let colors = self.default_colors.clone();
        let grapheme_cluster = self.default_grapheme_cluster;
        let cursor_default = (self.default_cursor_shape, self.default_cursor_blinking);
        let foreground = self.foreground_overridden.then_some(self.foreground);
        let background = self.background_overridden.then_some(self.background);
        let generation = self.generation;
        // Answers and requests already made still go, and a write in flight
        // goes on; the session's clipboard grants end, as Ghostty's do.
        let responses = std::mem::take(&mut self.responses);
        let events = std::mem::take(&mut self.events);
        let mut kitty_clipboard = std::mem::take(&mut self.kitty_clipboard);
        kitty_clipboard.grants.clear();
        *self = Self::new(cols, rows, limits);
        self.generation = generation.wrapping_add(1);
        self.responses = responses;
        self.events = events;
        self.kitty_clipboard = kitty_clipboard;
        self.set_default_cursor(cursor_default.0, cursor_default.1);
        self.default_grapheme_cluster = grapheme_cluster;
        self.modes.grapheme_cluster = grapheme_cluster;
        self.set_default_colors(colors);
        // Ghostty's RIS resets indexed colors but preserves OSC 10/11
        // dynamic overrides. Only OSC 110/111 clear those overrides.
        if let Some(color) = foreground {
            self.foreground = color;
            self.foreground_overridden = true;
        }
        if let Some(color) = background {
            self.background = color;
            self.background_overridden = true;
        }
    }
    /// Ghostty's `clear_screen`: the scrollback goes, then the whole screen
    /// when the cursor is on a marked shell prompt (true: the caller sends
    /// the shell a form feed so it draws its prompt again), else only the
    /// rows above the cursor, which moves to the top. A program's alternate
    /// screen is its own and stays as it is.
    pub fn clear_screen(&mut self) -> bool {
        if self.modes.alternate_screen {
            return false;
        }
        let at_prompt = self.semantic_markers_seen
            && self.primary.lines[self.primary.cursor.row].prompt != RowPrompt::None;
        let screen = &mut self.primary;
        screen.history.clear();
        if at_prompt {
            let cols = screen.cols;
            for line in &mut screen.lines {
                Arc::make_mut(line).reset(cols, Style::default());
            }
            screen.wrap_pending = false;
        } else if screen.cursor.row > 0 {
            // The whole screen moves, whatever scroll region or left and
            // right margins a program set.
            let count = screen.cursor.row;
            let region = (screen.top, screen.bottom, screen.left, screen.right);
            screen.top = 0;
            screen.bottom = screen.lines.len() - 1;
            screen.left = 0;
            screen.right = screen.cols - 1;
            screen.scroll_up(count, false, Style::default());
            (screen.top, screen.bottom, screen.left, screen.right) = region;
            screen.cursor.row = 0;
        }
        screen.enforce_budget();
        self.generation = self.generation.wrapping_add(1);
        at_prompt
    }
    pub fn set_default_cursor(&mut self, shape: CursorShape, blinking: bool) {
        self.default_cursor_shape = shape;
        self.default_cursor_blinking = blinking;
        if self.cursor_is_default {
            self.set_cursor_style(0);
        }
        self.generation = self.generation.wrapping_add(1);
    }
    fn set_cursor_style(&mut self, value: u16) {
        self.cursor_is_default = value == 0;
        self.cursor_blinking = if value == 0 {
            self.default_cursor_blinking
        } else {
            value % 2 == 1
        };
        self.screen_mut().cursor.shape = match value {
            0 => self.default_cursor_shape,
            3 | 4 => CursorShape::Underline,
            5 | 6 => CursorShape::Bar,
            _ => CursorShape::Block,
        };
    }
    fn cursor_style(&self) -> u16 {
        let base = match self.screen().cursor.shape {
            CursorShape::Block => 1,
            CursorShape::Underline => 3,
            CursorShape::Bar => 5,
        };
        base + u16::from(!self.cursor_blinking)
    }
    pub fn set_default_grapheme_width(&mut self, method: crate::GraphemeWidthMethod) {
        let enabled = method == crate::GraphemeWidthMethod::Unicode;
        self.default_grapheme_cluster = enabled;
        self.modes.grapheme_cluster = enabled;
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn set_default_colors(&mut self, colors: TerminalColors) {
        for (index, color) in self.palette.iter_mut().enumerate() {
            if !self.palette_overrides[index] {
                *color = colors.palette[index];
            }
        }
        if !self.foreground_overridden {
            self.foreground = colors.foreground;
        }
        if !self.background_overridden {
            self.background = colors.background;
        }
        self.default_colors = colors;
        self.generation = self.generation.wrapping_add(1);
    }
    fn screen(&self) -> &Screen {
        if self.modes.alternate_screen {
            &self.alternate
        } else {
            &self.primary
        }
    }
    fn screen_mut(&mut self) -> &mut Screen {
        if self.modes.alternate_screen {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.clamp(1, MAX_COLS);
        let rows = rows.clamp(1, MAX_ROWS);
        if cols == self.primary.cols && rows == self.primary.lines.len() {
            return;
        }
        if self.tabs.len() != cols {
            self.tabs = default_tabs(cols);
        }
        self.primary.resize(cols, rows, self.autowrap);
        self.alternate.resize(cols, rows, false);
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn scrollback_len(&self) -> usize {
        self.screen().history.len()
    }
    pub fn compress_history_step(&mut self) -> bool {
        self.primary
            .history
            .compress_step(self.primary.lines.len().saturating_mul(2))
    }
    pub fn history_memory(&self) -> crate::HistoryMemory {
        let screen = self.screen();
        debug_assert_eq!(
            screen.active_bytes(),
            screen.measure_active_bytes(),
            "active budget registration drift"
        );
        let mut memory = screen.history.memory();
        memory.active_bytes = screen.active_bytes();
        memory.limit_bytes = screen.byte_limit;
        memory
    }
    pub fn enforce_history_budget(&mut self) {
        self.primary.enforce_budget();
        self.alternate.enforce_budget();
    }
    pub fn keyboard_options(&self) -> KeyboardOptions {
        KeyboardOptions {
            application_cursor: self.modes.application_cursor,
            application_keypad: self.modes.application_keypad,
            backarrow: self.modes.backarrow,
            ignore_keypad_with_numlock: self.modes.ignore_keypad_with_numlock,
            alt_esc_prefix: self.modes.alt_esc_prefix,
            modify_other_keys: self.modes.modify_other_keys,
            kitty_flags: self.screen().kitty_keyboard.current(),
        }
    }
    pub fn mouse_modes(&self) -> (MouseTracking, MouseFormat) {
        (self.modes.mouse_tracking, self.modes.mouse_format)
    }
    pub fn modes(&self) -> Modes {
        Modes {
            kitty_flags: self.screen().kitty_keyboard.current(),
            ..self.modes
        }
    }
    pub fn content_position(&self, offset: usize) -> crate::ContentPosition {
        let screen = self.screen();
        screen.history.position(
            screen.history.len().saturating_sub(offset),
            screen.lines.len(),
        )
    }
    pub fn snapshot(&self, offset: usize) -> Snapshot {
        let screen = self.screen();
        let offset = offset.min(screen.history.len());
        let begin = screen.history.len() - offset;
        let lines = screen
            .history
            .range(begin, offset)
            .chain(screen.lines.iter().cloned())
            .take(screen.lines.len())
            .collect();
        let mut cursor = screen.cursor;
        cursor.row += offset;
        cursor.in_viewport = cursor.row < screen.lines.len();
        cursor.visible &= cursor.in_viewport;
        cursor.blinking = self.cursor_blinking;
        Snapshot {
            position: screen.history.position(begin, screen.lines.len()),
            cols: screen.cols,
            rows: screen.lines.len(),
            lines,
            cursor,
            palette: self.palette,
            default_foreground: self.foreground,
            default_background: self.background,
            modes: Modes {
                kitty_flags: screen.kitty_keyboard.current(),
                ..self.modes
            },
            generation: self.generation,
        }
    }
    pub fn snapshot_all(&self) -> Snapshot {
        let screen = self.screen();
        let mut snapshot = self.snapshot(0);
        snapshot.lines = screen
            .history
            .range(0, screen.history.len())
            .chain(screen.lines.iter().cloned())
            .collect();
        snapshot.rows = snapshot.lines.len();
        snapshot.position.first_row = 0;
        snapshot.cursor.row += screen.history.len();
        snapshot
    }
    pub fn content_snapshot(&self) -> crate::ContentSnapshot {
        crate::ContentSnapshot::capture(self.snapshot(0), self.screen().history.capture())
    }
    #[inline(never)]
    fn execute_horizontal(&mut self, byte: u8) {
        match byte {
            7 => self.event(TerminalEvent::Bell),
            8 => {
                let s = self.screen_mut();
                let left = if s.cursor.col < s.left { 0 } else { s.left };
                s.cursor.col = s.cursor.col.saturating_sub(1).max(left);
                s.wrap_pending = false;
            }
            9 => {
                let current = self.screen().cursor.col;
                let end = self.screen().right;
                if current >= end {
                    return;
                }
                let next = (current + 1..=end).find(|&i| self.tabs[i]).unwrap_or(end);
                let s = self.screen_mut();
                s.cursor.col = next;
            }
            10..=12 => {
                let style = self.style;
                let nl = self.newline;
                let origin = self.origin;
                let s = self.screen_mut();
                s.linefeed(style);
                if nl {
                    s.carriage_return(origin);
                }
            }
            13 => {
                let origin = self.origin;
                let s = self.screen_mut();
                s.carriage_return(origin);
            }
            14 => self.active_charset = 1,
            15 => self.active_charset = 0,
            _ => {}
        }
    }
    pub fn print_ascii(&mut self, mut bytes: &[u8]) {
        if self.insert
            || self.charset[self.active_charset] != 0
            || self.single_shift.is_some()
            || (self.left_right_margin_mode && !self.screen().full_width())
        {
            for &b in bytes {
                self.print(b as char);
            }
            return;
        }
        while !bytes.is_empty() {
            self.prepare_wrap();
            // Metadata allocations and releases must occur in scalar order:
            // otherwise the budget's peak (and retained history) depends on
            // where a PTY read happens to split an overwrite.
            if self.scalar_metadata_writes(bytes.len()) {
                let mut chars = bytes.iter().copied().map(char::from).peekable();
                let consumed = self.print_metadata_span(&mut chars);
                debug_assert!(consumed > 0);
                bytes = &bytes[consumed..];
                continue;
            }
            let style = self.style;
            let protected = self.protected;
            let s = self.screen_mut();
            let link = s.active_link.clone();
            let semantic = s.semantic;
            let col = s.cursor.col;
            let count = bytes.len().min(s.cols - col);
            let row = s.row();
            let clear_previous_spacer = col < 2
                && row.cells[col..(col + count).min(2)]
                    .iter()
                    .any(|c| c.width() != 1);
            // Repair only boundary-wide cells; the interior is fully replaced.
            row.prepare_write(col, col + count, style);
            let id = row.style_id(style);
            Cell::write_ascii(
                &mut row.cells[col..col + count],
                &bytes[..count],
                id,
                protected,
                semantic,
            );
            if let Some(link) = &link {
                row.set_hyperlink_range(col..col + count, Some(link));
            }
            if clear_previous_spacer {
                s.clear_previous_spacer();
            }
            s.cursor.col += count;
            if s.cursor.col == s.cols {
                s.cursor.col -= 1;
                s.wrap_pending = true;
            }
            s.enforce_budget();
            self.previous_char = Some(bytes[count - 1] as char);
            bytes = &bytes[count..];
        }
    }
    fn prepare_wrap(&mut self) {
        if self.screen().wrap_pending {
            self.wrap();
        }
    }
    // Rare beside the check: out of the print loops.
    #[cold]
    #[inline(never)]
    fn wrap(&mut self) {
        let auto = self.autowrap;
        let style = self.style;
        let s = self.screen_mut();
        s.wrap_pending = false;
        if auto {
            let mark_wrap = s.cursor.col == s.cols - 1;
            if mark_wrap {
                s.row().wrapped = true;
            }
            s.soft_linefeed(style);
            s.cursor.col = s.left;
            if mark_wrap {
                s.row().wrap_continuation = true;
            }
        }
    }
    /// `print` while DEC69 is set: the cursor's margins bound the line.
    #[inline(never)]
    fn print_within_margins(&mut self, mut ch: char) {
        // The reference stream ignores decoded C1 controls. They must not
        // consume a single shift or become REP's previous printed scalar.
        if matches!(ch, '\u{80}'..='\u{9f}') {
            return;
        }
        let original = ch;
        match self.charset[self.single_shift.take().unwrap_or(self.active_charset)] {
            1 => ch = dec_graphic(ch),
            2 if ch == '#' => ch = '£',
            _ => {}
        }
        if ch as u32 > 255 && self.modes.grapheme_cluster && self.try_grapheme_within_margins(ch) {
            return;
        }
        let width = char_width(ch);
        if width == 0 {
            if self.modes.grapheme_cluster {
                return;
            }
            let auto = self.autowrap;
            let s = self.screen_mut();
            if s.cursor.col == 0 && !(auto && s.wrap_pending) {
                return;
            }
            let mut col = if auto && s.wrap_pending {
                s.cursor.col
            } else {
                s.cursor.col - 1
            };
            if s.lines[s.cursor.row].cells[col].width() == 0 {
                col = col.saturating_sub(1);
            }
            let cell = s.lines[s.cursor.row].cells[col];
            if !cell.has_text()
                || (matches!(ch, '\u{fe0e}' | '\u{fe0f}')
                    && !crate::unicode::properties(cell.character()).extended_pictographic())
            {
                return;
            }
            s.row().combine(col, ch);
            s.enforce_budget();
            return;
        }
        self.previous_char = Some(original);
        self.prepare_wrap();
        let style = self.style;
        let insert = self.insert;
        let auto = self.autowrap;
        let protected = self.protected;
        let s = self.screen_mut();
        let link = s.active_link.clone();
        let semantic = s.semantic;
        let right_limit = s.right_limit();
        let mut width = width;
        if width == 2 && s.cursor.col + 1 >= right_limit {
            if auto && right_limit - s.left > 1 {
                let col = s.cursor.col;
                let mark_wrap = right_limit == s.cols;
                if mark_wrap {
                    s.row().erase(col, col + 1, style);
                    s.row().set_spacer_head(col, style);
                    s.row().wrapped = true;
                } else {
                    s.row()
                        .put_semantic(col, '\0', 1, style, protected, semantic);
                }
                s.row().cells[col] = s.row().cells[col].semantic(semantic);
                if let Some(link) = &link {
                    s.row().set_hyperlink_range(col..col + 1, Some(link));
                }
                s.soft_linefeed(style);
                s.cursor.col = s.left;
                if mark_wrap {
                    s.row().wrap_continuation = true;
                }
            } else if right_limit - s.left == 1 {
                ch = '\0';
                width = 1;
            } else {
                return;
            }
        }
        let col = s.cursor.col;
        if insert {
            if s.full_width() {
                s.row().shift_right(col, width, style);
            } else if s.inside_columns() {
                let right = s.right + 1;
                s.row().shift_margin(col, right, width, true, style);
            }
        }
        if col <= 1 && s.row().cells[col].width() != width && s.row().cells[col].width() != 1 {
            s.clear_previous_spacer();
        }
        s.row()
            .put_semantic(col, ch, width, style, protected, semantic);
        if let Some(link) = &link {
            s.row().set_hyperlink_range(col..col + width, Some(link));
        }
        s.cursor.col += width;
        if s.cursor.col >= right_limit {
            s.cursor.col = right_limit - 1;
            s.wrap_pending = true;
        }
    }
    /// Append a scalar only when the pinned terminal grapheme rules join it.
    /// Reconstructing the bounded cluster state also handles enabling 2027
    /// after legacy output that stored otherwise-invalid grapheme boundaries.
    fn try_grapheme(&mut self, ch: char) -> bool {
        use crate::unicode::{BreakState, WidthEffect, grapheme_break, width_effect};
        let screen = self.screen();
        if screen.cursor.col == 0 {
            return false;
        }
        let row = &screen.lines[screen.cursor.row];
        let left = if self.autowrap {
            usize::from(!screen.wrap_pending)
        } else if screen.cursor.col + 1 != screen.cols {
            1
        } else {
            usize::from(!row.cells[screen.cursor.col].has_text())
        };
        let mut col = screen.cursor.col - left;
        if row.cells[col].width() == 0 {
            col = col.saturating_sub(1);
        }
        let base = row.cells[col];
        if !base.has_text() {
            return false;
        }
        let mut previous = base.character();
        let mut state = BreakState::default();
        if let Some(text) = row.grapheme(col) {
            for next in text.chars().skip(1) {
                grapheme_break(previous, next, &mut state);
                previous = next;
            }
        }
        if grapheme_break(previous, ch, &mut state) {
            return false;
        }
        let effect = width_effect(previous, ch);
        if effect == WidthEffect::Ignore {
            return true;
        }
        let style = self.style;
        let protected = self.protected;
        let auto = self.autowrap;
        let screen = self.screen_mut();
        let semantic = screen.semantic;
        match effect {
            WidthEffect::Wide if base.width() != 2 => {
                screen.cursor.col = col;
                let link = screen.active_link.clone();
                if col + 1 == screen.cols {
                    if !auto {
                        return true;
                    }
                    let old = screen.lines[screen.cursor.row]
                        .grapheme(col)
                        .map(compact_str::CompactString::from);
                    screen.row().wrapped = true;
                    screen.row().set_spacer_head(col, style);
                    screen.row().cells[col] = screen.row().cells[col].semantic(semantic);
                    if let Some(link) = &link {
                        screen.row().set_hyperlink_range(col..col + 1, Some(link));
                    }
                    screen.cursor.col = 0;
                    screen.soft_linefeed(style);
                    screen.row().wrap_continuation = true;
                    col = 0;
                    screen
                        .row()
                        .put_semantic(col, base.character(), 2, style, protected, semantic);
                    if let Some(link) = &link {
                        screen.row().set_hyperlink_range(col..col + 2, Some(link));
                    }
                    if let Some(old) = old {
                        for scalar in old.chars().skip(1) {
                            screen.row().combine(col, scalar);
                        }
                    }
                } else {
                    if col == 0 && screen.lines[screen.cursor.row].cells[col + 1].width() == 2 {
                        screen.clear_previous_spacer();
                    }
                    screen
                        .row()
                        .widen_grapheme(col, style, protected, semantic, link.as_ref());
                }
                screen.cursor.col = (col + 2).min(screen.cols - 1);
                if col + 2 == screen.cols {
                    screen.wrap_pending = true;
                }
            }
            WidthEffect::Narrow if base.width() == 2 => {
                screen.row().narrow_grapheme(col);
                screen.wrap_pending = false;
                screen.cursor.col = (col + 1).min(screen.cols - 1);
            }
            _ => {}
        }
        screen.row().combine(col, ch);
        screen.enforce_budget();
        true
    }
    /// `try_grapheme` while DEC69 is set.
    #[inline(never)]
    fn try_grapheme_within_margins(&mut self, ch: char) -> bool {
        use crate::unicode::{BreakState, WidthEffect, grapheme_break, width_effect};
        let screen = self.screen();
        if screen.cursor.col == 0 {
            return false;
        }
        let row = &screen.lines[screen.cursor.row];
        let right_limit = screen.right_limit();
        let left = if self.autowrap {
            usize::from(!screen.wrap_pending)
        } else if screen.cursor.col + 1 != right_limit {
            1
        } else {
            usize::from(!row.cells[screen.cursor.col].has_text())
        };
        let mut col = screen.cursor.col - left;
        if row.cells[col].width() == 0 {
            col = col.saturating_sub(1);
        }
        let base = row.cells[col];
        if !base.has_text() {
            return false;
        }
        let mut previous = base.character();
        let mut state = BreakState::default();
        if let Some(text) = row.grapheme(col) {
            for next in text.chars().skip(1) {
                grapheme_break(previous, next, &mut state);
                previous = next;
            }
        }
        if grapheme_break(previous, ch, &mut state) {
            return false;
        }
        let effect = width_effect(previous, ch);
        if effect == WidthEffect::Ignore {
            return true;
        }
        let style = self.style;
        let protected = self.protected;
        let auto = self.autowrap;
        let screen = self.screen_mut();
        let semantic = screen.semantic;
        match effect {
            WidthEffect::Wide if base.width() != 2 => {
                screen.cursor.col = col;
                let link = screen.active_link.clone();
                if col + 1 == right_limit {
                    if !auto {
                        return true;
                    }
                    let old = screen.lines[screen.cursor.row]
                        .grapheme(col)
                        .map(compact_str::CompactString::from);
                    let mark_wrap = right_limit == screen.cols;
                    if mark_wrap {
                        screen.row().wrapped = true;
                        screen.row().set_spacer_head(col, style);
                    } else {
                        screen
                            .row()
                            .put_semantic(col, '\0', 1, style, protected, semantic);
                    }
                    screen.row().cells[col] = screen.row().cells[col].semantic(semantic);
                    if let Some(link) = &link {
                        screen.row().set_hyperlink_range(col..col + 1, Some(link));
                    }
                    screen.soft_linefeed(style);
                    if mark_wrap {
                        screen.row().wrap_continuation = true;
                    }
                    col = screen.left;
                    screen.cursor.col = col;
                    screen
                        .row()
                        .put_semantic(col, base.character(), 2, style, protected, semantic);
                    if let Some(link) = &link {
                        screen.row().set_hyperlink_range(col..col + 2, Some(link));
                    }
                    if let Some(old) = old {
                        for scalar in old.chars().skip(1) {
                            screen.row().combine(col, scalar);
                        }
                    }
                } else {
                    if col == 0 && screen.lines[screen.cursor.row].cells[col + 1].width() == 2 {
                        screen.clear_previous_spacer();
                    }
                    screen
                        .row()
                        .widen_grapheme(col, style, protected, semantic, link.as_ref());
                }
                screen.cursor.col = (col + 2).min(right_limit - 1);
                if col + 2 == right_limit {
                    screen.wrap_pending = true;
                }
            }
            WidthEffect::Narrow if base.width() == 2 => {
                screen.row().narrow_grapheme(col);
                screen.wrap_pending = false;
                screen.cursor.col = (col + 1).min(right_limit - 1);
            }
            _ => {}
        }
        screen.row().combine(col, ch);
        screen.enforce_budget();
        true
    }
    pub fn print_utf8(&mut self, text: &str) {
        if self.insert
            || self.charset[self.active_charset] != 0
            || self.single_shift.is_some()
            || (self.left_right_margin_mode && !self.screen().full_width())
        {
            for ch in text.chars() {
                self.print(ch);
            }
            return;
        }
        let mut chars = text.chars().peekable();
        while chars.peek().is_some() {
            if self.scalar_metadata_writes(usize::MAX) {
                self.print_metadata_span(&mut chars);
                if let Some(ch) = chars.next() {
                    self.print(ch);
                    self.screen_mut().enforce_budget();
                }
                continue;
            }
            let style = self.style;
            let protected = self.protected;
            let unicode = self.modes.grapheme_cluster;
            let auto = self.autowrap;
            let s = self.screen_mut();
            let mut col = s.cursor.col;
            let semantic = s.semantic;
            let link = s.active_link.clone();
            let mut pending = s.wrap_pending;
            let mut clear_previous_spacer = false;
            let mut previous = None;
            // One ownership check/copy per row, rather than an atomic Arc check
            // for every scalar. Controls cannot occur in a validated text run.
            let row = Arc::make_mut(&mut s.lines[s.cursor.row]);
            // Group positive-width writes until a combining mark or row
            // boundary. Intern style and clear overwritten metadata once per
            // run; combining and wrapping retain the existing scalar behavior.
            while let Some(&first) = chars.peek() {
                let mut ch = first;
                let mut width = char_width(ch);
                if width == 0 {
                    break;
                }
                if pending || col + width > s.cols {
                    break;
                }
                if unicode && !unicode_batch_boundary(row, col, auto, ch) {
                    break;
                }
                let start = col;
                let left = col > 0 && row.cells[col].width() == 0;
                let id = row.style_id(style);
                loop {
                    chars.next();
                    previous = Some(ch);
                    clear_previous_spacer |=
                        col <= 1 && row.cells[col].width() != width && row.cells[col].width() != 1;
                    row.cells[col] = Cell::new(ch, id, width as u8)
                        .protected(protected)
                        .semantic(semantic);
                    if width == 2 {
                        row.cells[col + 1] = Cell::new(' ', id, 0)
                            .protected(protected)
                            .semantic(semantic);
                    }
                    col += width;
                    if col == s.cols {
                        col -= 1;
                        pending = true;
                        break;
                    }
                    let Some(&next) = chars.peek() else {
                        break;
                    };
                    if unicode && !unicode_scalar_boundary(ch, next) {
                        break;
                    }
                    ch = next;
                    width = char_width(ch);
                    if width == 0 || col + width > s.cols {
                        break;
                    }
                }
                row.finish_print_run(
                    start,
                    if pending { s.cols } else { col },
                    left,
                    style,
                    link.as_ref(),
                );
            }
            s.cursor.col = col;
            s.wrap_pending = pending;
            if clear_previous_spacer {
                s.clear_previous_spacer();
            }
            s.enforce_budget();
            if let Some(previous) = previous {
                self.previous_char = Some(previous);
            }
            // The scalar at a row boundary goes through the normal wrapping
            // path, including one-column screens and disabled autowrap.
            if let Some(ch) = chars.next() {
                self.print(ch);
            }
        }
    }
    /// Preserve scalar allocation/link-table order while amortizing row COW
    /// checks over standalone positive-width scalars. Clusters and wrapping
    /// continue through the complete scalar state machine.
    fn print_metadata_span<I: Iterator<Item = char>>(
        &mut self,
        chars: &mut std::iter::Peekable<I>,
    ) -> usize {
        let style = self.style;
        let protected = self.protected;
        let unicode = self.modes.grapheme_cluster;
        let auto = self.autowrap;
        let screen = self.screen_mut();
        if screen.wrap_pending {
            return 0;
        }
        let link = screen.active_link.clone();
        let semantic = screen.semantic;
        let mut col = screen.cursor.col;
        let mut consumed = 0;
        let mut previous = None;
        let mut clear_previous_spacer = false;
        let row = Arc::make_mut(&mut screen.lines[screen.cursor.row]);
        while let Some(&ch) = chars.peek() {
            let width = char_width(ch);
            if width == 0
                || col + width > screen.cols
                || (unicode && !unicode_batch_boundary(row, col, auto, ch))
            {
                break;
            }
            chars.next();
            previous = Some(ch);
            clear_previous_spacer |=
                col <= 1 && row.cells[col].width() != width && row.cells[col].width() != 1;
            row.put_semantic(col, ch, width, style, protected, semantic);
            if let Some(link) = &link {
                row.set_hyperlink_range(col..col + width, Some(link));
            }
            col += width;
            consumed += 1;
            if col == screen.cols {
                screen.wrap_pending = true;
                col -= 1;
                break;
            }
        }
        screen.cursor.col = col;
        if clear_previous_spacer {
            screen.clear_previous_spacer();
        }
        // clear_spacer_head changes only cell flags, never metadata ownership;
        // deferring it does not alter the allocation peak or saved snapshots.
        // No scroll/history mutation occurs inside the span. Its highest
        // charged allocation is retained by BudgetSignal even after releases.
        screen.enforce_budget();
        if let Some(previous) = previous {
            self.previous_char = Some(previous);
        }
        consumed
    }
    #[inline(never)]
    fn print_repeat(&mut self, count: usize) {
        let Some(ch) = self.previous_char else { return };
        // Bounded stack storage amortizes the existing print fast paths without
        // allocating proportional to a control sequence's repeat count.
        let mut scalar = [0; 4];
        let encoded = ch.encode_utf8(&mut scalar);
        let mut bytes = [0; 1024];
        let batch = count.min(bytes.len() / encoded.len());
        let used = batch * encoded.len();
        for output in bytes[..used].chunks_exact_mut(encoded.len()) {
            output.copy_from_slice(encoded.as_bytes());
        }
        let text = std::str::from_utf8(&bytes[..used]).expect("repeated encoded scalar");
        let mut remaining = count;
        while remaining != 0 {
            let n = remaining.min(batch);
            if ch.is_ascii() {
                self.print_ascii(&bytes[..n]);
            } else {
                self.print_utf8(&text[..n * encoded.len()]);
            }
            remaining -= n;
        }
    }
    fn scalar_metadata_writes(&self, count: usize) -> bool {
        let screen = self.screen();
        let start = screen.cursor.col;
        let end = start + count.min(screen.cols - start);
        screen.active_link.is_some()
            || screen.lines[screen.cursor.row].needs_scalar_write_order(start..end, self.style)
    }
    fn move_to(&mut self, row: usize, col: usize) {
        let origin = self.origin;
        let s = self.screen_mut();
        s.cursor.row = if origin {
            (row + s.top).min(s.bottom)
        } else {
            row.min(s.lines.len() - 1)
        };
        s.cursor.col = if origin {
            (col + s.left).min(s.right)
        } else {
            col.min(s.cols - 1)
        };
        s.wrap_pending = false;
    }
    fn reply(&mut self, bytes: &[u8]) {
        self.responses.extend_from_slice(bytes);
    }
    fn event(&mut self, event: TerminalEvent) {
        // Coalesce UI state notifications. Clipboard transactions are ordered
        // separately; a later write must not replace an earlier query.
        self.events.retain(|existing| {
            !matches!(
                (existing, &event),
                (TerminalEvent::Bell, TerminalEvent::Bell)
                    | (TerminalEvent::Title(_), TerminalEvent::Title(_))
                    | (
                        TerminalEvent::WorkingDirectory(_),
                        TerminalEvent::WorkingDirectory(_)
                    )
                    | (
                        TerminalEvent::Notification { .. },
                        TerminalEvent::Notification { .. }
                    )
            )
        });
        self.events.push(event);
    }
    fn clipboard_event(&mut self, body: &[u8]) {
        let (selection, data) = match body {
            [b';', data @ ..] => (b'c', data),
            [selection, b';', data @ ..] if *selection != b';' => (*selection, data),
            _ => return,
        };
        let (count, bytes) = self.clipboard_prefix();
        if count >= MAX_CLIPBOARD_EVENTS || data.len() > MAX_CLIPBOARD_BYTES.saturating_sub(bytes) {
            // A read past the bounds is answered empty, so the program
            // asking doesn't wait for an answer that won't come.
            if data == b"?" {
                self.reply(&[b"\x1b]52;", &[selection][..], b";\x1b\\"].concat());
            }
            return;
        }
        let mut owned = Vec::new();
        if owned.try_reserve_exact(data.len()).is_err()
            || owned.capacity() > MAX_CLIPBOARD_BYTES.saturating_sub(bytes)
            || self.events.try_reserve(1).is_err()
        {
            return;
        }
        owned.extend_from_slice(data);
        self.events.push(TerminalEvent::Clipboard {
            selection,
            data: owned,
        });
    }
    /// Clipboard requests waiting in this feed's events, and the OSC 52 bytes
    /// they hold. Kitty writes are bounded by their transaction's limit.
    fn clipboard_prefix(&self) -> (usize, usize) {
        self.events
            .iter()
            .fold((0usize, 0usize), |(count, bytes), event| match event {
                TerminalEvent::Clipboard { data, .. } => {
                    (count + 1, bytes.saturating_add(data.capacity()))
                }
                TerminalEvent::KittyClipboard(_) => (count + 1, bytes),
                _ => (count, bytes),
            })
    }
    fn kitty_clipboard_event(&mut self, body: &[u8], bell_terminated: bool) {
        use crate::kitty_clipboard::Terminator;
        let (metadata, payload) = match body.iter().position(|&b| b == b';') {
            Some(at) => (&body[..at], Some(&body[at + 1..])),
            None => (body, None),
        };
        let terminator = if bell_terminated {
            Terminator::Bel
        } else {
            Terminator::St
        };
        let Some(request) =
            self.kitty_clipboard
                .packet(metadata, payload, terminator, &mut self.responses)
        else {
            return;
        };
        // In order with OSC 52's, in the same bounded prefix. Past it the
        // program hears EBUSY rather than waiting for an answer.
        if self.clipboard_prefix().0 >= MAX_CLIPBOARD_EVENTS || self.events.try_reserve(1).is_err()
        {
            request.busy(&mut self.responses);
            return;
        }
        self.events.push(TerminalEvent::KittyClipboard(request));
    }
    pub fn kitty_clipboard_grant(
        &mut self,
        pw: &str,
        direction: crate::kitty_clipboard::Direction,
    ) {
        self.kitty_clipboard.grants.grant(pw, direction, false);
    }
    pub fn kitty_paste_event(
        &mut self,
        primary: bool,
        available: &[String],
        random: &[u8],
    ) -> Option<Vec<u8>> {
        if !self.modes.kitty_paste_events {
            return None;
        }
        self.kitty_clipboard.paste_event(primary, available, random)
    }
    pub fn set_kitty_clipboard_write_limit(&mut self, bytes: usize) {
        self.kitty_clipboard.write_limit = bytes;
    }
    fn semantic_prompt(&mut self, data: &[u8]) {
        let Some(&action) = data.first() else { return };
        if !matches!(
            action,
            b'A' | b'B' | b'C' | b'D' | b'I' | b'L' | b'N' | b'P'
        ) || (data.len() > 1 && (action == b'L' || data[1] != b';'))
        {
            return;
        }
        self.semantic_markers_seen = true;
        // Options are byte strings; malformed UTF-8 in an unknown option does
        // not invalidate the command. Prompt kind affects row navigation, not
        // the per-cell classification implemented here.
        let left = if self.screen().cursor.col < self.screen().left {
            0
        } else {
            self.screen().left
        };
        if matches!(action, b'A' | b'N' | b'L') && self.screen().cursor.col != left {
            let style = self.style;
            let origin = self.origin;
            let screen = self.screen_mut();
            screen.carriage_return(origin);
            screen.linefeed(style);
        }
        let screen = self.screen_mut();
        screen.semantic = match action {
            b'A' | b'N' | b'P' => SemanticContent::Prompt,
            b'B' | b'I' => SemanticContent::Input,
            b'C' | b'D' => SemanticContent::Output,
            _ => return,
        };
        screen.semantic_clear_eol = action == b'I';
        if screen.semantic == SemanticContent::Prompt {
            let kind = data
                .get(2..)
                .unwrap_or_default()
                .split(|&b| b == b';')
                .find_map(|field| field.strip_prefix(b"k="));
            screen.row().prompt = if matches!(kind, Some(b"c" | b"s")) {
                RowPrompt::Continuation
            } else {
                RowPrompt::Prompt
            };
        } else if action == b'C' && screen.cursor.col == 0 {
            screen.row().prompt = RowPrompt::None;
        }
    }
    fn save_cursor(&mut self) {
        let saved = SavedCursor {
            cursor: self.screen().cursor,
            style: self.style,
            origin: self.origin,
            wrap_pending: self.screen().wrap_pending,
            charset: self.charset,
            active_charset: self.active_charset,
            protected: self.protected,
        };
        self.screen_mut().saved = saved;
        self.screen_mut().saved_valid = true;
    }
    fn restore_cursor(&mut self) {
        let saved = self.screen().saved;
        self.style = saved.style;
        self.origin = saved.origin;
        self.charset = saved.charset;
        self.active_charset = saved.active_charset;
        self.single_shift = None;
        self.protected = saved.protected;
        let s = self.screen_mut();
        let visible = s.cursor.visible;
        let shape = s.cursor.shape;
        s.cursor = saved.cursor;
        s.cursor.visible = visible;
        s.cursor.shape = shape;
        s.cursor.col = s.cursor.col.min(s.cols - 1);
        s.cursor.row = s.cursor.row.min(s.lines.len() - 1);
        s.wrap_pending = saved.wrap_pending;
    }
    fn clear_alternate_screen(&mut self, style: Style) {
        debug_assert!(self.modes.alternate_screen);
        let screen = self.screen_mut();
        // Alternate history is empty, but its identity must change when the
        // screen is cleared so retained Find anchors cannot refer to a new use.
        screen.history.clear();
        for row in &mut screen.lines {
            Arc::make_mut(row).reset(screen.cols, style);
        }
        screen.wrap_pending = false;
    }
    #[inline(never)]
    fn switch_screen_mode(&mut self, value: u16, set: bool) {
        // These transitions are intentionally not idempotent: 1049 saves and
        // clears on every enable and restores on every disable. 1047 clears
        // the alternate screen on exit; 47 never clears either screen.
        if value == 1049 && set {
            self.save_cursor();
        }
        if value == 1047 && !set && self.modes.alternate_screen {
            self.clear_alternate_screen(self.style);
        }
        let changed = self.modes.alternate_screen != set;
        let old_cursor = self.screen().cursor;
        let old_wrap = self.screen().wrap_pending;
        let old_semantic = self.screen().semantic;
        let old_clear_eol = self.screen().semantic_clear_eol;
        let destination_style = if changed && set {
            self.alternate.inactive_style
        } else {
            self.style
        };
        if changed {
            self.screen_mut().inactive_style = self.style;
            let top = self.screen().top;
            let bottom = self.screen().bottom;
            let left = self.screen().left;
            let right = self.screen().right;
            self.screen_mut().set_active_link(None);
            self.modes.alternate_screen = set;
            let screen = self.screen_mut();
            let shape = screen.cursor.shape;
            screen.cursor = old_cursor;
            if value == 1049 && !set {
                screen.cursor.shape = shape;
            }
            screen.wrap_pending = old_wrap;
            if value != 1049 || set {
                screen.semantic = old_semantic;
                screen.semantic_clear_eol = old_clear_eol;
            }
            // Scroll margins are terminal state shared by the two screens.
            screen.top = top;
            screen.bottom = bottom;
            screen.left = left;
            screen.right = right;
        }
        if value == 1049 {
            if set {
                self.clear_alternate_screen(destination_style);
                if changed {
                    let screen = self.screen_mut();
                    screen.cursor = old_cursor;
                    screen.wrap_pending = old_wrap;
                }
            } else {
                self.restore_cursor();
            }
        }
    }
    fn mode(&mut self, private: bool, value: u16, set: bool) {
        if !private {
            match value {
                4 => self.insert = set,
                20 => self.newline = set,
                _ => {}
            }
            return;
        }
        match value {
            1 => self.modes.application_cursor = set,
            66 => self.modes.application_keypad = set,
            67 => self.modes.backarrow = set,
            69 => {
                self.left_right_margin_mode = set;
                if !set {
                    let screen = self.screen_mut();
                    screen.left = 0;
                    screen.right = screen.cols - 1;
                }
            }
            1035 => self.modes.ignore_keypad_with_numlock = set,
            1036 => self.modes.alt_esc_prefix = set,
            9 | 1000 | 1002 | 1003 => {
                let (bit, mode) = match value {
                    9 => (1, MouseTracking::X10),
                    1000 => (2, MouseTracking::Normal),
                    1002 => (4, MouseTracking::Button),
                    _ => (8, MouseTracking::Any),
                };
                if set {
                    self.mouse_tracking_modes |= bit;
                } else {
                    self.mouse_tracking_modes &= !bit;
                }
                self.modes.mouse_tracking = if set { mode } else { MouseTracking::None };
            }
            1005 | 1006 | 1015 | 1016 => {
                let (bit, format) = match value {
                    1005 => (1, MouseFormat::Utf8),
                    1006 => (2, MouseFormat::Sgr),
                    1015 => (4, MouseFormat::Urxvt),
                    _ => (8, MouseFormat::SgrPixels),
                };
                if set {
                    self.mouse_format_modes |= bit;
                } else {
                    self.mouse_format_modes &= !bit;
                }
                self.modes.mouse_format = if set { format } else { MouseFormat::X10 };
            }
            1007 => self.modes.mouse_alternate_scroll = set,
            6 => {
                self.origin = set;
                self.move_to(0, 0);
            }
            7 => self.autowrap = set,
            12 => self.cursor_blinking = set,
            25 => self.screen_mut().cursor.visible = set,
            47 | 1047 | 1049 => {
                let bit = match value {
                    47 => 1,
                    1047 => 2,
                    _ => 4,
                };
                if set {
                    self.alternate_mode_bits |= bit;
                } else {
                    self.alternate_mode_bits &= !bit;
                }
                self.switch_screen_mode(value, set);
            }
            1048 => {
                self.save_cursor_mode = set;
                if set {
                    self.save_cursor()
                } else {
                    self.restore_cursor()
                }
            }
            1004 => self.modes.focus_events = set,
            2004 => self.modes.bracketed_paste = set,
            2027 => self.modes.grapheme_cluster = set,
            5522 => self.modes.kitty_paste_events = set,
            _ => {}
        }
    }
    fn mode_status(&self, private: bool, value: u16) -> u8 {
        // Recognized fixed-reset modes have no mutable state or side effects.
        if (private && value == 117)
            || (!private
                && matches!(
                    value,
                    1 | 5 | 7 | 10 | 11 | 13 | 14 | 15 | 16 | 17 | 18 | 19
                ))
        {
            return 4;
        }
        let set = if private {
            match value {
                1 => Some(self.modes.application_cursor),
                66 => Some(self.modes.application_keypad),
                67 => Some(self.modes.backarrow),
                69 => Some(self.left_right_margin_mode),
                1035 => Some(self.modes.ignore_keypad_with_numlock),
                1036 => Some(self.modes.alt_esc_prefix),
                9 | 1000 | 1002 | 1003 => Some(
                    self.mouse_tracking_modes
                        & match value {
                            9 => 1,
                            1000 => 2,
                            1002 => 4,
                            _ => 8,
                        }
                        != 0,
                ),
                1005 | 1006 | 1015 | 1016 => Some(
                    self.mouse_format_modes
                        & match value {
                            1005 => 1,
                            1006 => 2,
                            1015 => 4,
                            _ => 8,
                        }
                        != 0,
                ),
                1007 => Some(self.modes.mouse_alternate_scroll),
                6 => Some(self.origin),
                7 => Some(self.autowrap),
                12 => Some(self.cursor_blinking),
                25 => Some(self.screen().cursor.visible),
                47 | 1047 | 1049 => Some(
                    self.alternate_mode_bits
                        & match value {
                            47 => 1,
                            1047 => 2,
                            _ => 4,
                        }
                        != 0,
                ),
                1048 => Some(self.save_cursor_mode),
                1004 => Some(self.modes.focus_events),
                2004 => Some(self.modes.bracketed_paste),
                2027 => Some(self.modes.grapheme_cluster),
                5522 => Some(self.modes.kitty_paste_events),
                _ => None,
            }
        } else {
            match value {
                4 => Some(self.insert),
                20 => Some(self.newline),
                _ => None,
            }
        };
        match set {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        }
    }
    fn alignment_pattern(&mut self) {
        // DECALN keeps the pen's foreground/background, but clears its other
        // SGR attributes. The cursor's protection, link and semantic state
        // survive; the pattern itself carries none of those attributes.
        self.style = Style {
            foreground: self.style.foreground,
            background: self.style.background,
            ..Style::default()
        };
        self.origin = false;
        let style = self.style;
        let screen = self.screen_mut();
        screen.top = 0;
        screen.bottom = screen.lines.len() - 1;
        screen.left = 0;
        screen.right = screen.cols - 1;
        screen.cursor.row = 0;
        screen.cursor.col = 0;
        screen.wrap_pending = false;
        for index in 0..screen.lines.len() {
            let row = Arc::make_mut(&mut screen.lines[index]);
            row.reset(screen.cols, style);
            row.prepare_write(0, screen.cols, style);
            let id = row.style_id(style);
            row.cells.fill(Cell::new('E', id, 1));
            screen.enforce_budget();
        }
    }
    fn soft_reset(&mut self) {
        self.modes.application_cursor = false;
        self.modes.application_keypad = false;
        self.modes.modify_other_keys = false;
        self.origin = false;
        self.autowrap = true;
        self.left_right_margin_mode = false;
        self.insert = false;
        self.style = Style::default();
        self.protected = false;
        self.charset = [0; 4];
        self.active_charset = 0;
        self.single_shift = None;
        self.set_cursor_style(0);
        let screen = self.screen_mut();
        screen.cursor.visible = true;
        screen.top = 0;
        screen.bottom = screen.lines.len() - 1;
        screen.left = 0;
        screen.right = screen.cols - 1;
        screen.saved = SavedCursor::default();
        screen.saved_valid = false;
        self.palette = self.default_colors.palette;
        self.palette_overrides = [false; 256];
    }
    fn reply_color(&mut self, command: &str, color: Rgb, bell_terminated: bool) {
        let Rgb(r, g, b) = color;
        let end = if bell_terminated { "\x07" } else { "\x1b\\" };
        self.reply(
            format!(
                "\x1b]{command};rgb:{:04x}/{:04x}/{:04x}{end}",
                u16::from(r) * 257,
                u16::from(g) * 257,
                u16::from(b) * 257
            )
            .as_bytes(),
        );
    }
    fn sgr(&mut self, params: &[Param]) {
        let mut i = 0;
        while i < params.len() {
            let mut colon_end = i;
            while colon_end + 1 < params.len() && params[colon_end].colon {
                colon_end += 1;
            }
            if params[i].colon && !matches!(params[i].value, 4 | 38 | 48 | 58) {
                i = colon_end + 1;
                continue;
            }
            match params[i].value {
                0 => self.style = Style::default(),
                1 => self.style.attrs |= attrs::BOLD,
                2 => self.style.attrs |= attrs::DIM,
                3 => self.style.attrs |= attrs::ITALIC,
                4 => {
                    if params[i].colon {
                        if colon_end == i + 1 && !params[i + 1].colon {
                            self.style.set_underline(match params[i + 1].value {
                                0 => Underline::None,
                                2 => Underline::Double,
                                3 => Underline::Curly,
                                4 => Underline::Dotted,
                                5 => Underline::Dashed,
                                _ => Underline::Single,
                            });
                        }
                        i = colon_end;
                    } else {
                        self.style.set_underline(Underline::Single);
                    }
                }
                5 | 6 => self.style.attrs |= attrs::BLINK,
                7 => self.style.attrs |= attrs::INVERSE,
                8 => self.style.attrs |= attrs::HIDDEN,
                9 => self.style.attrs |= attrs::STRIKE,
                21 => self.style.set_underline(Underline::Double),
                22 => self.style.attrs &= !(attrs::BOLD | attrs::DIM),
                23 => self.style.attrs &= !attrs::ITALIC,
                24 => self.style.set_underline(Underline::None),
                25 => self.style.attrs &= !attrs::BLINK,
                27 => self.style.attrs &= !attrs::INVERSE,
                28 => self.style.attrs &= !attrs::HIDDEN,
                29 => self.style.attrs &= !attrs::STRIKE,
                53 => self.style.attrs |= attrs::OVERLINE,
                55 => self.style.attrs &= !attrs::OVERLINE,
                n @ 30..=37 => self.style.foreground = Color::Indexed((n - 30) as u8),
                n @ 40..=47 => self.style.background = Color::Indexed((n - 40) as u8),
                n @ 90..=97 => self.style.foreground = Color::Indexed((n - 90 + 8) as u8),
                n @ 100..=107 => self.style.background = Color::Indexed((n - 100 + 8) as u8),
                39 => self.style.foreground = Color::Default,
                49 => self.style.background = Color::Default,
                59 => self.style.underline_color = Color::Default,
                which @ (38 | 48 | 58) => {
                    let mut color = None;
                    if let Some(kind) = params.get(i + 1) {
                        if kind.value == 5 && i + 2 < params.len() {
                            color = Some(Color::Indexed(params[i + 2].value as u8));
                            i += 2;
                        } else if kind.value == 2 && i + 4 < params.len() {
                            let extra = usize::from(params[i].colon && colon_end - i == 5);
                            let start = i + 2 + extra;
                            if !params[i].colon || matches!(colon_end - i, 4 | 5) {
                                let [r, g, b] = [
                                    params[start].value,
                                    params[start + 1].value,
                                    params[start + 2].value,
                                ];
                                color = Some(Color::Rgb(Rgb(r as u8, g as u8, b as u8)));
                                i = start + 2;
                            } else {
                                i = colon_end;
                            }
                        }
                    }
                    if let Some(color) = color {
                        match which {
                            38 => self.style.foreground = color,
                            48 => self.style.background = color,
                            _ => self.style.underline_color = color,
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

impl Handler for State {
    fn print(&mut self, mut ch: char) {
        // With DEC69 off the margins are the screen's edges, and this,
        // the pre-margin path, stays the hot one, as `execute` splits too.
        if self.left_right_margin_mode {
            self.print_within_margins(ch);
            return;
        }
        // The reference stream ignores decoded C1 controls. They must not
        // consume a single shift or become REP's previous printed scalar.
        if matches!(ch, '\u{80}'..='\u{9f}') {
            return;
        }
        let original = ch;
        match self.charset[self.single_shift.take().unwrap_or(self.active_charset)] {
            1 => ch = dec_graphic(ch),
            2 if ch == '#' => ch = '£',
            _ => {}
        }
        if ch as u32 > 255 && self.modes.grapheme_cluster && self.try_grapheme(ch) {
            return;
        }
        let width = char_width(ch);
        if width == 0 {
            if self.modes.grapheme_cluster {
                return;
            }
            let auto = self.autowrap;
            let s = self.screen_mut();
            if s.cursor.col == 0 && !(auto && s.wrap_pending) {
                return;
            }
            let mut col = if auto && s.wrap_pending {
                s.cursor.col
            } else {
                s.cursor.col - 1
            };
            if s.lines[s.cursor.row].cells[col].width() == 0 {
                col = col.saturating_sub(1);
            }
            let cell = s.lines[s.cursor.row].cells[col];
            if !cell.has_text()
                || (matches!(ch, '\u{fe0e}' | '\u{fe0f}')
                    && !crate::unicode::properties(cell.character()).extended_pictographic())
            {
                return;
            }
            s.row().combine(col, ch);
            s.enforce_budget();
            return;
        }
        self.previous_char = Some(original);
        self.prepare_wrap();
        let style = self.style;
        let insert = self.insert;
        let auto = self.autowrap;
        let protected = self.protected;
        let s = self.screen_mut();
        let link = s.active_link.clone();
        let semantic = s.semantic;
        let mut width = width;
        if width == 2 && s.cursor.col + 1 >= s.cols {
            if auto && s.cols > 1 {
                let col = s.cursor.col;
                s.row().erase(col, col + 1, style);
                s.row().set_spacer_head(col, style);
                s.row().cells[col] = s.row().cells[col].semantic(semantic);
                if let Some(link) = &link {
                    s.row().set_hyperlink_range(col..col + 1, Some(link));
                }
                s.row().wrapped = true;
                s.cursor.col = 0;
                s.soft_linefeed(style);
                s.row().wrap_continuation = true;
            } else if s.cols == 1 {
                ch = '\0';
                width = 1;
            } else {
                return;
            }
        }
        let col = s.cursor.col;
        if insert {
            s.row().shift_right(col, width, style);
        }
        if col <= 1 && s.row().cells[col].width() != width && s.row().cells[col].width() != 1 {
            s.clear_previous_spacer();
        }
        s.row()
            .put_semantic(col, ch, width, style, protected, semantic);
        if let Some(link) = &link {
            s.row().set_hyperlink_range(col..col + width, Some(link));
        }
        s.cursor.col += width;
        if s.cursor.col >= s.cols {
            s.cursor.col = s.cols - 1;
            s.wrap_pending = true;
        }
    }
    fn execute(&mut self, byte: u8) {
        // Mode reset, resize and screen switches keep full-width margins when
        // DEC69 is disabled. Keep its coordinate work out of the common C0 path.
        if self.left_right_margin_mode {
            self.execute_horizontal(byte);
            return;
        }
        debug_assert!(self.screen().full_width());
        match byte {
            7 => self.event(TerminalEvent::Bell),
            8 => {
                let s = self.screen_mut();
                s.cursor.col = s.cursor.col.saturating_sub(1);
                s.wrap_pending = false;
            }
            9 => {
                let current = self.screen().cursor.col;
                let end = self.screen().cols - 1;
                let next = (current + 1..=end).find(|&i| self.tabs[i]).unwrap_or(end);
                let s = self.screen_mut();
                s.cursor.col = next;
            }
            10..=12 => {
                let style = self.style;
                let nl = self.newline;
                let s = self.screen_mut();
                s.linefeed_in_columns(style, true);
                if nl {
                    s.cursor.col = 0;
                }
            }
            13 => {
                let s = self.screen_mut();
                s.cursor.col = 0;
                s.wrap_pending = false;
            }
            14 => self.active_charset = 1,
            15 => self.active_charset = 0,
            _ => {}
        }
    }
    fn esc(&mut self, intermediates: &[u8], final_byte: u8) {
        if intermediates == b"#" && final_byte == b'8' {
            self.alignment_pattern();
            return;
        }
        if let [which @ (b'(' | b')' | b'*' | b'+')] = intermediates {
            self.charset[(*which - b'(') as usize] = match final_byte {
                b'0' => 1,
                b'A' => 2,
                _ => 0,
            };
            return;
        }
        if !intermediates.is_empty() {
            return;
        }
        match final_byte {
            b'N' => self.single_shift = Some(2),
            b'O' => self.single_shift = Some(3),
            b'n' => self.active_charset = 2,
            b'o' => self.active_charset = 3,
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'D' => {
                let style = self.style;
                self.screen_mut().linefeed(style);
            }
            b'E' => {
                let style = self.style;
                let origin = self.origin;
                let s = self.screen_mut();
                s.carriage_return(origin);
                s.linefeed(style);
            }
            b'M' => {
                let style = self.style;
                let s = self.screen_mut();
                // Scrolling keeps a pending wrap, as the reference's scroll
                // down does; moving up clears it.
                if s.cursor.row == s.top && s.inside_columns() {
                    s.scroll_down(1, style);
                } else {
                    s.wrap_pending = false;
                    let top = if s.cursor.row < s.top { 0 } else { s.top };
                    s.cursor.row = s.cursor.row.saturating_sub(1).max(top);
                }
            }
            b'H' => {
                let col = self.screen().cursor.col;
                self.tabs[col] = true;
            }
            b'=' => self.modes.application_keypad = true,
            b'>' => self.modes.application_keypad = false,
            b'Z' => self.reply(b"\x1b[?62;22c"),
            b'c' => self.full_reset(),
            _ => {}
        }
    }
    fn csi(&mut self, csi: Csi<'_>) {
        let p = |i: usize, default: usize| -> usize {
            csi.params
                .get(i)
                .map(|p| p.value as usize)
                .filter(|&n| n != 0)
                .unwrap_or(default)
        };
        let first = csi.params.first().map_or(0, |p| p.value);
        if !csi.intermediates.is_empty() {
            match (csi.intermediates, csi.final_byte, csi.private) {
                (b"!", b'p', None) if !csi.has_parameters => self.soft_reset(),
                (b"\"", b'q', None) if csi.params.len() == 1 && first <= 2 => {
                    self.protected = first == 1
                }
                (b" ", b'q', None) if csi.params.len() == 1 && first <= 6 => {
                    self.set_cursor_style(first)
                }
                (b"$", b'p', None | Some(b'?')) if csi.has_parameters && csi.params.len() == 1 => {
                    let status = self.mode_status(csi.private.is_some(), first);
                    let prefix = if csi.private.is_some() { "?" } else { "" };
                    self.reply(format!("\x1b[{prefix}{first};{status}$y").as_bytes());
                }
                _ => {}
            }
            return;
        }
        if matches!(csi.final_byte, b'h' | b'l') && matches!(csi.private, None | Some(b'?')) {
            for param in csi.params {
                self.mode(
                    csi.private == Some(b'?'),
                    param.value,
                    csi.final_byte == b'h',
                );
            }
            return;
        }
        if csi.final_byte == b'u' && csi.private.is_some() {
            let count = if csi.has_parameters {
                csi.params.len()
            } else {
                0
            };
            match csi.private {
                Some(b'?') => self.reply(
                    format!("\x1b[?{}u", self.screen().kitty_keyboard.current().0).as_bytes(),
                ),
                Some(b'>') => {
                    let flags = if count == 1 { first } else { 0 };
                    if flags <= 31 {
                        self.screen_mut()
                            .kitty_keyboard
                            .push(KittyFlags(flags as u8));
                    }
                }
                Some(b'<') => self.screen_mut().kitty_keyboard.pop(if count == 1 {
                    first as usize
                } else {
                    1
                }),
                Some(b'=') if first <= 31 => {
                    let mode = if count >= 2 { csi.params[1].value } else { 1 };
                    self.screen_mut()
                        .kitty_keyboard
                        .set(KittyFlags(first as u8), mode);
                }
                _ => {}
            }
            return;
        }
        if csi.private == Some(b'?') && matches!(csi.final_byte, b's' | b'r') {
            for param in csi.params {
                if csi.final_byte == b's' {
                    self.saved_modes
                        .set(param.value, self.mode_status(true, param.value) == 1);
                } else if let Some(saved) = self.saved_modes.get(param.value) {
                    self.mode(true, param.value, saved);
                }
            }
            return;
        }
        if csi.final_byte == b'm' && csi.private == Some(b'>') {
            if csi.params.len() <= 2 && matches!(first, 0 | 1 | 2 | 4) {
                self.modes.modify_other_keys =
                    first == 4 && csi.params.get(1).is_some_and(|p| p.value == 2);
            }
            return;
        }
        if csi.final_byte == b'n' && csi.private == Some(b'>') {
            self.modes.modify_other_keys = false;
            return;
        }
        if csi.final_byte == b'q' && csi.private == Some(b'>') {
            self.reply(format!("\x1bP>|Hotaru {}\x1b\\", env!("CARGO_PKG_VERSION")).as_bytes());
            return;
        }
        if csi.private.is_some()
            && !matches!(
                (csi.private, csi.final_byte),
                (Some(b'?'), b'n' | b'J' | b'K' | b'W') | (Some(b'>') | Some(b'='), b'c')
            )
        {
            return;
        }
        // The reference ignores these commands when they carry more
        // parameters than they take.
        let count = if csi.has_parameters {
            csi.params.len()
        } else {
            0
        };
        // A match, not a slice search: this runs for every CSI, SGR included.
        let most = match csi.final_byte {
            b'H' | b'f' | b'r' => 2,
            b'@' | b'A'..=b'G' | b'I'..=b'M' | b'P' | b'S' | b'T' | b'W' | b'X' | b'Z' => 1,
            b'`' | b'a' | b'd' | b'e' | b'g' | b'j' | b'k' | b'n' => 1,
            _ => usize::MAX,
        };
        if count > most {
            return;
        }
        // An explicit zero is no operation for these; the cursor movements
        // and ICH/ECH treat it as one.
        if count == 1
            && first == 0
            && matches!(
                csi.final_byte,
                b'I' | b'L' | b'M' | b'P' | b'S' | b'T' | b'Z'
            )
        {
            return;
        }
        let style = self.style;
        let origin = self.origin;
        match csi.final_byte {
            b'b' if csi.params.len() == 1 && !csi.params[0].colon => self.print_repeat(p(0, 1)),
            // HPB and VPB move like CUB and CUU.
            b'A' | b'B' | b'C' | b'D' | b'E' | b'F' | b'j' | b'k' => {
                let s = self.screen_mut();
                let n = p(0, 1);
                let top = if s.cursor.row >= s.top { s.top } else { 0 };
                let bottom = if s.cursor.row <= s.bottom {
                    s.bottom
                } else {
                    s.lines.len() - 1
                };
                let left = if s.cursor.col >= s.left { s.left } else { 0 };
                let right = if s.cursor.col <= s.right {
                    s.right
                } else {
                    s.cols - 1
                };
                match csi.final_byte {
                    b'A' | b'F' | b'k' => s.cursor.row = s.cursor.row.saturating_sub(n).max(top),
                    b'B' | b'E' => s.cursor.row = (s.cursor.row + n).min(bottom),
                    b'C' => s.cursor.col = (s.cursor.col + n).min(right),
                    b'D' | b'j' => s.cursor.col = s.cursor.col.saturating_sub(n).max(left),
                    _ => {}
                }
                if matches!(csi.final_byte, b'E' | b'F') {
                    s.carriage_return(origin);
                }
                s.wrap_pending = false;
            }
            b'a' | b'e' => {
                // These position-relative controls use the same coordinate
                // conversion as CUP, including both axes of a restored cursor
                // that lies outside newly established origin margins.
                // An explicit zero moves nothing but still re-sets the
                // position, clearing a pending wrap.
                let n = if count == 0 { 1 } else { usize::from(first) };
                let s = self.screen();
                let row = s.cursor.row.saturating_sub(if origin { s.top } else { 0 });
                let col = s.cursor.col.saturating_sub(if origin { s.left } else { 0 });
                if csi.final_byte == b'a' {
                    self.move_to(row, col + n);
                } else {
                    self.move_to(row + n, col);
                }
            }
            b'G' | b'`' => {
                let s = self.screen();
                let row = s.cursor.row.saturating_sub(if origin { s.top } else { 0 });
                self.move_to(row, p(0, 1) - 1);
            }
            b'H' | b'f' => self.move_to(p(0, 1) - 1, p(1, 1) - 1),
            b'd' => {
                let col = self.screen().cursor.col.saturating_sub(if origin {
                    self.screen().left
                } else {
                    0
                });
                self.move_to(p(0, 1) - 1, col);
            }
            b'J' => {
                // Prompt-marked ED2 preserves prior content in scrollback,
                // including when the shell first homes the cursor.
                if first == 2 && self.semantic_markers_seen && !self.modes.alternate_screen {
                    let at_prompt = self
                        .screen()
                        .lines
                        .iter()
                        .rev()
                        .find(|row| row.prompt != RowPrompt::None || row.has_text())
                        .is_some_and(|row| row.prompt != RowPrompt::None);
                    if at_prompt {
                        let screen = self.screen_mut();
                        let count = screen
                            .lines
                            .iter()
                            .rposition(|row| {
                                row.cells.iter().any(|cell| {
                                    cell.has_text()
                                        || (cell.background_only()
                                            && row.style(*cell).background != Color::Default)
                                })
                            })
                            .map_or(0, |row| row + 1);
                        let top = screen.top;
                        let bottom = screen.bottom;
                        let left = screen.left;
                        let right = screen.right;
                        screen.top = 0;
                        screen.bottom = screen.lines.len() - 1;
                        screen.left = 0;
                        screen.right = screen.cols - 1;
                        screen.scroll_up(count, true, Style::default());
                        screen.top = top;
                        screen.bottom = bottom;
                        screen.left = left;
                        screen.right = right;
                        if screen.cursor.row < count {
                            screen.cursor.col = 0;
                            screen.cursor.row = 0;
                        } else {
                            screen.cursor.row -= count;
                        }
                    }
                }
                let s = self.screen_mut();
                let col = s.cursor.col;
                let row = s.cursor.row;
                let cols = s.cols;
                if csi.private == Some(b'?') {
                    for r in 0..s.lines.len() {
                        let range = match first {
                            0 if r >= row => Some((if r == row { col } else { 0 }, cols)),
                            1 if r <= row => Some((0, if r == row { col + 1 } else { cols })),
                            2 => Some((0, cols)),
                            _ => None,
                        };
                        if let Some((start, end)) = range {
                            Arc::make_mut(&mut s.lines[r]).erase_selective(start, end, style);
                        }
                    }
                    s.wrap_pending = false;
                    return;
                }
                match first {
                    0 => {
                        s.row().erase(col, cols, style);
                        for r in row + 1..s.lines.len() {
                            Arc::make_mut(&mut s.lines[r]).reset(cols, style);
                        }
                    }
                    1 => {
                        for r in 0..row {
                            Arc::make_mut(&mut s.lines[r]).reset(cols, style);
                        }
                        s.row().erase(0, col + 1, style);
                    }
                    2 => {
                        for line in &mut s.lines {
                            Arc::make_mut(line).reset(cols, style);
                        }
                    }
                    3 => s.history.clear(),
                    _ => {}
                }
                s.wrap_pending = false;
            }
            b'K' => {
                let s = self.screen_mut();
                let col = s.cursor.col;
                let cols = s.cols;
                if matches!(first, 0 | 2) && s.row().wrapped {
                    s.row().wrapped = false;
                    let next = s.cursor.row + 1;
                    if let Some(row) = s.lines.get_mut(next) {
                        Arc::make_mut(row).wrap_continuation = false;
                    }
                    if s.row().cells[cols - 1].is_spacer_head() {
                        s.row().erase(cols - 1, cols, style);
                    }
                }
                if csi.private == Some(b'?') {
                    let range = match first {
                        0 => Some((col, cols)),
                        1 => Some((0, col + 1)),
                        2 => Some((0, cols)),
                        _ => None,
                    };
                    if let Some((start, end)) = range {
                        s.row().erase_selective(start, end, style);
                    }
                    s.wrap_pending = false;
                    return;
                }
                match first {
                    0 => s.row().erase(col, cols, style),
                    1 => s.row().erase(0, col + 1, style),
                    2 => s.row().erase(0, cols, style),
                    _ => {}
                }
                s.wrap_pending = false;
            }
            b'X' => {
                let s = self.screen_mut();
                let col = s.cursor.col;
                let end = (col + p(0, 1)).min(s.cols);
                s.row().erase(col, end, style);
                s.wrap_pending = false;
            }
            b'@' | b'P' => {
                let s = self.screen_mut();
                let col = s.cursor.col;
                if !s.inside_columns() {
                    if csi.final_byte == b'@' {
                        s.wrap_pending = false;
                    }
                    return;
                }
                if !s.full_width() {
                    let right = s.right + 1;
                    s.row()
                        .shift_margin(col, right, p(0, 1), csi.final_byte == b'@', style);
                } else if csi.final_byte == b'@' {
                    s.row().shift_right(col, p(0, 1), style);
                } else {
                    s.row().shift_left(col, p(0, 1), style);
                }
                s.wrap_pending = false;
            }
            b'L' | b'M' => {
                let s = self.screen_mut();
                if s.cursor.row >= s.top && s.cursor.row <= s.bottom && s.inside_columns() {
                    let top = s.top;
                    s.top = s.cursor.row;
                    if csi.final_byte == b'L' {
                        s.scroll_down(p(0, 1), style);
                    } else {
                        s.scroll_up(p(0, 1), false, style);
                    }
                    s.top = top;
                    s.cursor.col = s.left;
                    s.wrap_pending = false;
                }
            }
            b'S' => self.screen_mut().scroll_up(p(0, 1), true, style),
            b'T' => self.screen_mut().scroll_down(p(0, 1), style),
            b'r' => {
                let s = self.screen_mut();
                let top = p(0, 1) - 1;
                let bottom = p(1, s.lines.len()) - 1;
                if top < bottom && bottom < s.lines.len() {
                    s.top = top;
                    s.bottom = bottom;
                    self.move_to(0, 0);
                }
            }
            b'm' => self.sgr(csi.params),
            b's' => {
                if self.left_right_margin_mode && csi.params.len() <= 2 {
                    let left = p(0, 1) - 1;
                    let right = p(1, self.screen().cols).min(self.screen().cols) - 1;
                    if left < right {
                        let s = self.screen_mut();
                        s.left = left;
                        s.right = right;
                        self.move_to(0, 0);
                    }
                } else if !self.left_right_margin_mode && !csi.has_parameters {
                    self.save_cursor();
                }
            }
            b'u' => self.restore_cursor(),
            b'g' => {
                let col = self.screen().cursor.col;
                match first {
                    0 => self.tabs[col] = false,
                    3 => self.tabs.fill(false),
                    _ => {}
                }
            }
            // CTC sets or clears tab stops; DECST8C restores one every eight
            // columns.
            b'W' => {
                let col = self.screen().cursor.col;
                match (csi.private, first) {
                    (None, 0) => self.tabs[col] = true,
                    (None, 2) => self.tabs[col] = false,
                    (None, 5) => self.tabs.fill(false),
                    (Some(b'?'), 5) => self.tabs = default_tabs(self.tabs.len()),
                    _ => {}
                }
            }
            b'I' => {
                for _ in 0..p(0, 1).min(self.screen().cols) {
                    self.execute(9);
                }
            }
            b'Z' => {
                for _ in 0..p(0, 1).min(self.screen().cols) {
                    let col = self.screen().cursor.col;
                    let left = if origin { self.screen().left } else { 0 };
                    if col <= left {
                        break;
                    }
                    let previous = (left..col).rev().find(|&c| self.tabs[c]).unwrap_or(left);
                    let s = self.screen_mut();
                    s.cursor.col = previous;
                    s.wrap_pending = false;
                }
            }
            b'n' => match first {
                5 if csi.private.is_none() => self.reply(b"\x1b[0n"),
                6 => {
                    let s = self.screen();
                    // A saved cursor can be restored above newly changed
                    // margins. Ghostty preserves that absolute position and
                    // clamps the origin-relative report to row one.
                    let row = s
                        .cursor
                        .row
                        .saturating_sub(if self.origin { s.top } else { 0 })
                        + 1;
                    let prefix = if csi.private.is_some() { "?" } else { "" };
                    let page = if csi.private.is_some() { ";1" } else { "" };
                    self.reply(
                        format!(
                            "\x1b[{prefix}{row};{}{page}R",
                            s.cursor.col.saturating_sub(if origin { s.left } else { 0 }) + 1
                        )
                        .as_bytes(),
                    );
                }
                _ => {}
            },
            b'c' => match csi.private {
                Some(b'>') => self.reply(b"\x1b[>1;0;0c"),
                Some(b'=') => self.reply(b"\x1bP!|00000000\x1b\\"),
                _ => self.reply(b"\x1b[?62;22c"),
            },
            _ => {}
        }
    }
    fn osc(&mut self, bytes: &[u8], bell_terminated: bool) {
        if let Some(body) = bytes.strip_prefix(b"52;") {
            self.clipboard_event(body);
            return;
        }
        if let Some(body) = bytes.strip_prefix(b"5522;") {
            self.kitty_clipboard_event(body, bell_terminated);
            return;
        }
        if let Some(body) = bytes.strip_prefix(b"133;") {
            self.semantic_prompt(body);
            return;
        }
        if let Some(body) = bytes.strip_prefix(b"8;") {
            // The reference OSC 8 parser reserves one fixed-buffer byte for
            // its sentinel NUL. Invalid commands retain the active link.
            if body.len() > 2047 {
                return;
            }
            let Some(separator) = body.iter().position(|&b| b == b';') else {
                return;
            };
            let mut id = None;
            let parameters = &body[..separator];
            let mut start = 0;
            while start < parameters.len() {
                // Upstream searches for the delimiter after the first byte.
                // An empty segment is joined with the next option as an
                // unknown key; later well-formed options are still parsed.
                let end = parameters[start + 1..]
                    .iter()
                    .position(|&b| b == b':')
                    .map_or(parameters.len(), |offset| start + 1 + offset);
                let parameter = &parameters[start..end];
                let Some(equal) = parameter.iter().position(|&b| b == b'=') else {
                    break;
                };
                if &parameter[..equal] == b"id" && equal + 1 < parameter.len() {
                    id = Some(&parameter[equal + 1..]);
                }
                start = end + 1;
            }
            let uri = &body[separator + 1..];
            if uri.is_empty() {
                if id.is_none() {
                    self.screen_mut().set_active_link(None);
                }
            } else {
                let id = if let Some(id) = id {
                    LinkIdentity::Explicit(id.into())
                } else {
                    let id = self.next_link_id;
                    self.next_link_id = self.next_link_id.wrapping_add(1);
                    LinkIdentity::Implicit(id)
                };
                self.screen_mut()
                    .set_active_link(Some(LinkRef::new(Hyperlink {
                        id,
                        uri: uri.into(),
                    })));
            }
            return;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        let (command, data) = text.split_once(';').unwrap_or((text, ""));
        match command {
            "0" | "2" => self.event(TerminalEvent::Title(data.chars().take(4096).collect())),
            "7" if data.len() <= 16 * 1024 => {
                self.event(TerminalEvent::WorkingDirectory(data.to_owned()));
            }
            "9" if !conemu_command(data) => self.event(TerminalEvent::Notification {
                title: String::new(),
                body: data.to_owned(),
            }),
            "777" => {
                if let Some(rest) = data.strip_prefix("notify;") {
                    let (title, body) = rest.split_once(';').unwrap_or((rest, ""));
                    self.event(TerminalEvent::Notification {
                        title: title.to_owned(),
                        body: body.to_owned(),
                    });
                }
            }
            "4" => {
                // Ghostty/xterm ignore empty fields and stop at the first
                // invalid pair, keeping operations that preceded that pair.
                let mut parts = data.split(';').filter(|part| !part.is_empty());
                while let (Some(index), Some(value)) = (parts.next(), parts.next()) {
                    let Ok(index) = index.parse::<u8>() else {
                        break;
                    };
                    if value == "?" {
                        self.reply_color(
                            &format!("4;{index}"),
                            self.palette[index as usize],
                            bell_terminated,
                        );
                    } else if let Some(color) = parse_color(value) {
                        self.palette[index as usize] = color;
                        self.palette_overrides[index as usize] = true;
                    } else {
                        break;
                    }
                }
            }
            "10" => {
                if data == "?" {
                    self.reply_color("10", self.foreground, bell_terminated);
                } else if let Some(color) = parse_color(data) {
                    self.foreground = color;
                    self.foreground_overridden = true;
                }
            }
            "11" => {
                if data == "?" {
                    self.reply_color("11", self.background, bell_terminated);
                } else if let Some(color) = parse_color(data) {
                    self.background = color;
                    self.background_overridden = true;
                }
            }
            "104" => {
                if data.is_empty() {
                    self.palette = self.default_colors.palette;
                    self.palette_overrides.fill(false);
                } else {
                    for index in data.split(';').filter_map(|s| s.parse::<u8>().ok()) {
                        self.palette[index as usize] = self.default_colors.palette[index as usize];
                        self.palette_overrides[index as usize] = false;
                    }
                }
            }
            "110" => {
                self.foreground = self.default_colors.foreground;
                self.foreground_overridden = false;
            }
            "111" => {
                self.background = self.default_colors.background;
                self.background_overridden = false;
            }
            _ => {}
        }
    }
    fn dcs(&mut self, header: Csi<'_>, bytes: &[u8]) {
        // The reference buffers at most two request bytes and drops a longer
        // request without replying.
        if header.private.is_some()
            || header.intermediates != b"$"
            || header.final_byte != b'q'
            || bytes.len() > 2
        {
            return;
        }
        let reply = match bytes {
            b"m" => Some(self.style.sgr()),
            b"r" => Some(format!(
                "{};{}r",
                self.screen().top + 1,
                self.screen().bottom + 1
            )),
            b"s" if self.left_right_margin_mode => Some(format!(
                "{};{}s",
                self.screen().left + 1,
                self.screen().right + 1
            )),
            b" q" => Some(format!("{} q", self.cursor_style())),
            _ => None,
        };
        if let Some(reply) = reply {
            self.reply(format!("\x1bP1$r{reply}\x1b\\").as_bytes());
        } else {
            self.reply(b"\x1bP0$r\x1b\\");
        }
    }
}

/// ConEmu's OSC 9 commands (sleep, message box, tab title, progress, wait,
/// macro), which share the number with iTerm2's notification text.
fn conemu_command(data: &str) -> bool {
    let bytes = data.as_bytes();
    matches!(bytes.first(), Some(b'1'..=b'6')) && matches!(bytes.get(1), None | Some(b';'))
}

fn parse_color(value: &str) -> Option<Rgb> {
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let n = u32::from_str_radix(hex, 16).ok()?;
        return Some(Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8));
    }
    let mut p = value.strip_prefix("rgb:")?.split('/');
    let component = |v: &str| -> Option<u8> {
        if v.is_empty() || v.len() > 4 {
            return None;
        }
        let n = u32::from_str_radix(v, 16).ok()?;
        Some((n * 255 / ((1u32 << (v.len() * 4)) - 1)) as u8)
    };
    let color = Rgb(
        component(p.next()?)?,
        component(p.next()?)?,
        component(p.next()?)?,
    );
    if p.next().is_some() {
        None
    } else {
        Some(color)
    }
}

/// A tab stop every eight columns, the power-on and DECST8C default.
fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|col| col != 0 && col % 8 == 0).collect()
}

#[inline]
fn han(ch: char) -> bool {
    matches!(ch, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}')
}
/// A scalar's width in both width modes, as the reference terminal prints it:
/// one cell up to U+00FF, the pinned uucode table above that.
#[inline]
fn char_width(ch: char) -> usize {
    if ch as u32 <= 255 {
        // End a batch at decoded C1; the scalar dispatch ignores it without
        // changing cursor, grapheme, charset or REP state.
        usize::from(!matches!(ch, '\u{80}'..='\u{9f}'))
    } else if han(ch) {
        2
    } else {
        crate::unicode::properties(ch).width()
    }
}
#[inline]
fn unicode_scalar_boundary(previous: char, next: char) -> bool {
    next as u32 <= 255
        || (han(previous) && han(next))
        || crate::unicode::grapheme_break(
            previous,
            next,
            &mut crate::unicode::BreakState::default(),
        )
}
fn unicode_batch_boundary(row: &Row, col: usize, auto: bool, ch: char) -> bool {
    if ch as u32 <= 255 || col == 0 {
        return true;
    }
    let left = if auto || col + 1 < row.cells.len() {
        1
    } else {
        usize::from(!row.cells[col].has_text())
    };
    let mut previous = col - left;
    if row.cells[previous].width() == 0 {
        previous = previous.saturating_sub(1);
    }
    let cell = row.cells[previous];
    !cell.has_text()
        || (row.grapheme(previous).is_none() && unicode_scalar_boundary(cell.character(), ch))
}

fn dec_graphic(ch: char) -> char {
    match ch {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => ch,
    }
}

#[cfg(test)]
mod width_fastpath_test {
    use super::*;

    #[test]
    fn width_fastpaths_match_pinned_properties_for_every_scalar() {
        for value in 0..=0x10ffff {
            if let Some(ch) = char::from_u32(value) {
                let expected = if matches!(ch, '\u{80}'..='\u{9f}') {
                    // Stream-dispatch sentinel: the actual-library controls
                    // corpus checks that decoded C1 is ignored in every path.
                    0
                } else if value <= 255 {
                    1
                } else {
                    crate::unicode::properties(ch).width()
                };
                assert_eq!(char_width(ch), expected, "U+{value:04X}");
            }
        }
    }
}
