use crate::history::History;
use crate::hyperlink::{Hyperlink, LinkIdentity, LinkRef};
use crate::input::{KeyboardOptions, KittyFlags, KittyKeyboard, MouseFormat, MouseTracking};
use crate::parser::{Csi, Handler, Param};
use crate::style::Underline;
use crate::{Cell, Color, MAX_COLS, MAX_ROWS, Rgb, Row, Snapshot, Style, TerminalColors, attrs};
use std::collections::VecDeque;
use triomphe::Arc;
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub visible: bool,
}
impl Default for Cursor {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            visible: true,
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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    Title(String),
    Bell,
    /// Base64 text from OSC 52, not an instruction to accept clipboard access.
    /// The application applies its policy before decoding or requesting data.
    Clipboard {
        selection: String,
        data: String,
    },
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

struct Screen {
    lines: VecDeque<Arc<Row>>,
    history: History,
    max_history: usize,
    cols: usize,
    cursor: Cursor,
    saved: SavedCursor,
    saved_valid: bool,
    wrap_pending: bool,
    top: usize,
    bottom: usize,
    kitty_keyboard: KittyKeyboard,
    active_link: Option<LinkRef>,
}

impl Screen {
    fn new(cols: usize, rows: usize, max_history: usize) -> Self {
        Self {
            lines: (0..rows).map(|_| Arc::new(Row::new(cols))).collect(),
            history: History::default(),
            max_history,
            cols,
            cursor: Cursor::default(),
            saved: SavedCursor::default(),
            saved_valid: false,
            wrap_pending: false,
            top: 0,
            bottom: rows - 1,
            kitty_keyboard: KittyKeyboard::default(),
            active_link: None,
        }
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
        for _ in 0..count.min(self.bottom - self.top + 1) {
            let full_screen = self.top == 0 && self.bottom + 1 == self.lines.len();
            let old = self.lines.remove(self.top).expect("valid scroll region");
            let recycled = if history && full_screen && self.max_history > 0 {
                self.history.push_back(old);
                if self.history.len() > self.max_history {
                    self.history.pop_front_recycle(self.cols)
                } else {
                    None
                }
            } else {
                Some(old)
            };
            let mut blank = recycled.unwrap_or_else(|| Arc::new(Row::new(self.cols)));
            Arc::make_mut(&mut blank).reset(self.cols, style);
            self.lines.insert(self.bottom, blank);
        }
    }
    fn scroll_down(&mut self, count: usize, style: Style) {
        for _ in 0..count.min(self.bottom - self.top + 1) {
            let mut line = self.lines.remove(self.bottom).expect("valid scroll region");
            Arc::make_mut(&mut line).reset(self.cols, style);
            self.lines.insert(self.top, line);
        }
    }
    fn linefeed(&mut self, style: Style) {
        self.wrap_pending = false;
        if self.cursor.row == self.bottom {
            self.scroll_up(1, true, style);
        } else {
            self.cursor.row = (self.cursor.row + 1).min(self.lines.len() - 1);
        }
    }
    fn resize(&mut self, cols: usize, rows: usize, reflow: bool) {
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
            limit: self.max_history + old_rows.max(rows),
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
            let mut source_col = 0;
            while source_col < len {
                if pending {
                    Arc::make_mut(self.lines.back_mut().unwrap()).wrapped = true;
                    self.push_blank(true);
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
    mouse_tracking_modes: u8,
    mouse_format_modes: u8,
    style: Style,
    origin: bool,
    autowrap: bool,
    insert: bool,
    newline: bool,
    charset: [u8; 4],
    active_charset: usize,
    single_shift: Option<usize>,
    protected: bool,
    cursor_style: u16,
    next_link_id: u64,
    tabs: Vec<bool>,
    default_colors: TerminalColors,
    palette: [Rgb; 256],
    palette_overrides: [bool; 256],
    foreground: Rgb,
    background: Rgb,
    foreground_overridden: bool,
    background_overridden: bool,
    pub generation: u64,
    pub responses: Vec<u8>,
    pub events: Vec<TerminalEvent>,
}

impl State {
    pub fn new(cols: usize, rows: usize, history: usize) -> Self {
        let cols = cols.clamp(1, MAX_COLS);
        let rows = rows.clamp(1, MAX_ROWS);
        // Bound retained cells as well as row count. A pathological wide PTY
        // cannot silently request gigabytes of history from a row-count setting.
        let history = history.min(8_000_000 / cols);
        Self {
            primary: Screen::new(cols, rows, history),
            alternate: Screen::new(cols, rows, 0),
            modes: Modes::default(),
            mouse_tracking_modes: 0,
            mouse_format_modes: 0,
            style: Style::default(),
            origin: false,
            autowrap: true,
            insert: false,
            newline: false,
            charset: [0; 4],
            active_charset: 0,
            single_shift: None,
            protected: false,
            cursor_style: 1,
            next_link_id: 0,
            tabs: (0..cols).map(|c| c != 0 && c % 8 == 0).collect(),
            default_colors: TerminalColors::default(),
            palette: crate::style::default_palette(),
            palette_overrides: [false; 256],
            foreground: Rgb(229, 229, 229),
            background: Rgb(0, 0, 0),
            foreground_overridden: false,
            background_overridden: false,
            generation: 0,
            responses: Vec::new(),
            events: Vec::new(),
        }
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
            self.tabs = (0..cols).map(|col| col != 0 && col % 8 == 0).collect();
        }
        self.primary.max_history = self.primary.max_history.min(8_000_000 / cols);
        while self.primary.history.len() > self.primary.max_history {
            self.primary.history.pop_front();
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
        self.screen().history.memory()
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
        cursor.visible &= offset == 0;
        Snapshot {
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
        snapshot.cursor.row += screen.history.len();
        snapshot
    }
    pub fn content_snapshot(&self) -> crate::ContentSnapshot {
        crate::ContentSnapshot::capture(self.snapshot(0), self.screen().history.capture())
    }
    pub fn print_ascii(&mut self, mut bytes: &[u8]) {
        if self.insert || self.charset[self.active_charset] != 0 || self.single_shift.is_some() {
            for &b in bytes {
                self.print(b as char);
            }
            return;
        }
        while !bytes.is_empty() {
            self.prepare_wrap();
            let style = self.style;
            let protected = self.protected;
            let s = self.screen_mut();
            let link = s.active_link.clone();
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
            for (cell, &byte) in row.cells[col..col + count].iter_mut().zip(&bytes[..count]) {
                *cell = Cell::new(byte as char, id, 1).protected(protected);
            }
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
            bytes = &bytes[count..];
        }
    }
    fn prepare_wrap(&mut self) {
        if self.screen().wrap_pending {
            let auto = self.autowrap;
            let style = self.style;
            let s = self.screen_mut();
            s.wrap_pending = false;
            if auto {
                s.row().wrapped = true;
                s.cursor.col = 0;
                s.linefeed(style);
                s.row().wrap_continuation = true;
            }
        }
    }
    pub fn print_utf8(&mut self, text: &str) {
        if self.insert || self.charset[self.active_charset] != 0 || self.single_shift.is_some() {
            for ch in text.chars() {
                self.print(ch);
            }
            return;
        }
        let mut chars = text.chars().peekable();
        while chars.peek().is_some() {
            let style = self.style;
            let protected = self.protected;
            let s = self.screen_mut();
            let mut col = s.cursor.col;
            let link = s.active_link.clone();
            let mut pending = s.wrap_pending;
            let mut clear_previous_spacer = false;
            // One ownership check/copy per row, rather than an atomic Arc check
            // for every scalar. Controls cannot occur in a validated text run.
            let row = Arc::make_mut(&mut s.lines[s.cursor.row]);
            while let Some(&ch) = chars.peek() {
                let width = char_width(ch);
                if width != 0 && (pending || col + width > s.cols) {
                    break;
                }
                chars.next();
                if width == 0 {
                    row.combine(if pending { col } else { col.saturating_sub(1) }, ch);
                } else {
                    clear_previous_spacer |=
                        col <= 1 && row.cells[col].width() != width && row.cells[col].width() != 1;
                    row.put(col, ch, width, style, protected);
                    if let Some(link) = &link {
                        row.set_hyperlink_range(col..col + width, Some(link));
                    }
                    col += width;
                    if col == s.cols {
                        col -= 1;
                        pending = true;
                    }
                }
            }
            s.cursor.col = col;
            s.wrap_pending = pending;
            if clear_previous_spacer {
                s.clear_previous_spacer();
            }
            // The scalar at a row boundary goes through the normal wrapping
            // path, including one-column screens and disabled autowrap.
            if let Some(ch) = chars.next() {
                self.print(ch);
            }
        }
    }
    fn move_to(&mut self, row: usize, col: usize) {
        let origin = self.origin;
        let s = self.screen_mut();
        s.cursor.row = if origin {
            (row + s.top).min(s.bottom)
        } else {
            row.min(s.lines.len() - 1)
        };
        s.cursor.col = col.min(s.cols - 1);
        s.wrap_pending = false;
    }
    fn reply(&mut self, bytes: &[u8]) {
        self.responses.extend_from_slice(bytes);
    }
    fn event(&mut self, event: TerminalEvent) {
        // One pending notification of each kind. Bell floods must not consume
        // slots needed by a later title or clipboard-policy notification.
        self.events.retain(|existing| {
            !matches!(
                (existing, &event),
                (TerminalEvent::Bell, TerminalEvent::Bell)
                    | (TerminalEvent::Title(_), TerminalEvent::Title(_))
                    | (
                        TerminalEvent::Clipboard { .. },
                        TerminalEvent::Clipboard { .. }
                    )
            )
        });
        self.events.push(event);
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
        s.cursor = saved.cursor;
        s.cursor.col = s.cursor.col.min(s.cols - 1);
        s.cursor.row = s.cursor.row.min(s.lines.len() - 1);
        s.wrap_pending = saved.wrap_pending;
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
            25 => self.screen_mut().cursor.visible = set,
            47 | 1047 | 1049 => {
                if self.modes.alternate_screen != set {
                    self.screen_mut().active_link = None;
                }
                if self.modes.alternate_screen == set {
                    return;
                }
                if set {
                    if value == 1049 {
                        self.save_cursor();
                    }
                    let cols = self.primary.cols;
                    let rows = self.primary.lines.len();
                    let cursor = self.primary.cursor;
                    if value != 47 {
                        let kitty_keyboard = self.alternate.kitty_keyboard;
                        self.alternate = Screen::new(cols, rows, 0);
                        self.alternate.kitty_keyboard = kitty_keyboard;
                    }
                    self.alternate.cursor = cursor;
                    self.modes.alternate_screen = true;
                } else {
                    self.primary.cursor = self.alternate.cursor;
                    self.modes.alternate_screen = false;
                    if value == 1049 {
                        self.restore_cursor();
                    }
                }
            }
            1048 => {
                if set {
                    self.save_cursor()
                } else {
                    self.restore_cursor()
                }
            }
            1004 => self.modes.focus_events = set,
            2004 => self.modes.bracketed_paste = set,
            _ => {}
        }
    }
    fn mode_status(&self, private: bool, value: u16) -> u8 {
        let set = if private {
            match value {
                1 => Some(self.modes.application_cursor),
                66 => Some(self.modes.application_keypad),
                67 => Some(self.modes.backarrow),
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
                25 => Some(self.screen().cursor.visible),
                47 | 1047 | 1049 => Some(self.modes.alternate_screen),
                1004 => Some(self.modes.focus_events),
                2004 => Some(self.modes.bracketed_paste),
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
    fn soft_reset(&mut self) {
        self.modes.application_cursor = false;
        self.modes.application_keypad = false;
        self.modes.modify_other_keys = false;
        self.origin = false;
        self.autowrap = true;
        self.insert = false;
        self.style = Style::default();
        self.protected = false;
        self.charset = [0; 4];
        self.active_charset = 0;
        self.single_shift = None;
        self.cursor_style = 1;
        let screen = self.screen_mut();
        screen.cursor.visible = true;
        screen.top = 0;
        screen.bottom = screen.lines.len() - 1;
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
        match self.charset[self.single_shift.take().unwrap_or(self.active_charset)] {
            1 => ch = dec_graphic(ch),
            2 if ch == '#' => ch = '£',
            _ => {}
        }
        // Ghostty's default (non-grapheme-clustering) width policy treats each
        // regional indicator as a wide scalar, including an unpaired one.
        let width = char_width(ch);
        if width == 0 {
            let s = self.screen_mut();
            let col = if s.wrap_pending {
                s.cursor.col
            } else {
                s.cursor.col.saturating_sub(1)
            };
            s.row().combine(col, ch);
            return;
        }
        self.prepare_wrap();
        let style = self.style;
        let insert = self.insert;
        let auto = self.autowrap;
        let protected = self.protected;
        let s = self.screen_mut();
        let link = s.active_link.clone();
        let mut width = width;
        if width == 2 && s.cursor.col + 1 >= s.cols {
            if auto && s.cols > 1 {
                let col = s.cursor.col;
                s.row().erase(col, col + 1, style);
                s.row().set_spacer_head(col, style);
                if let Some(link) = &link {
                    s.row().set_hyperlink_range(col..col + 1, Some(link));
                }
                s.row().wrapped = true;
                s.cursor.col = 0;
                s.linefeed(style);
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
        s.row().put(col, ch, width, style, protected);
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
                s.linefeed(style);
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
                let s = self.screen_mut();
                s.cursor.col = 0;
                s.linefeed(style);
            }
            b'M' => {
                let style = self.style;
                let s = self.screen_mut();
                s.wrap_pending = false;
                if s.cursor.row == s.top {
                    s.scroll_down(1, style);
                } else {
                    s.cursor.row = s.cursor.row.saturating_sub(1);
                }
            }
            b'H' => {
                let col = self.screen().cursor.col;
                self.tabs[col] = true;
            }
            b'=' => self.modes.application_keypad = true,
            b'>' => self.modes.application_keypad = false,
            b'Z' => self.reply(b"\x1b[?62;22c"),
            b'c' => {
                let cols = self.screen().cols;
                let rows = self.screen().lines.len();
                let history = self.primary.max_history;
                let colors = self.default_colors.clone();
                let foreground = self.foreground_overridden.then_some(self.foreground);
                let background = self.background_overridden.then_some(self.background);
                *self = Self::new(cols, rows, history);
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
                    self.cursor_style = first.max(1)
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
                (Some(b'?'), b'n' | b'J' | b'K') | (Some(b'>') | Some(b'='), b'c')
            )
        {
            return;
        }
        let style = self.style;
        let origin = self.origin;
        match csi.final_byte {
            b'A' | b'B' | b'C' | b'D' | b'E' | b'F' | b'a' | b'e' => {
                let s = self.screen_mut();
                let n = p(0, 1);
                let top = if origin { s.top } else { 0 };
                let bottom = if origin { s.bottom } else { s.lines.len() - 1 };
                match csi.final_byte {
                    b'A' | b'F' => s.cursor.row = s.cursor.row.saturating_sub(n).max(top),
                    b'B' | b'E' | b'e' => s.cursor.row = (s.cursor.row + n).min(bottom),
                    b'C' | b'a' => s.cursor.col = (s.cursor.col + n).min(s.cols - 1),
                    b'D' => s.cursor.col = s.cursor.col.saturating_sub(n),
                    _ => {}
                }
                if matches!(csi.final_byte, b'E' | b'F') {
                    s.cursor.col = 0;
                }
                s.wrap_pending = false;
            }
            b'G' | b'`' => {
                let s = self.screen_mut();
                s.cursor.col = (p(0, 1) - 1).min(s.cols - 1);
                s.wrap_pending = false;
            }
            b'H' | b'f' => self.move_to(p(0, 1) - 1, p(1, 1) - 1),
            b'd' => {
                let col = self.screen().cursor.col;
                self.move_to(p(0, 1) - 1, col);
            }
            b'J' => {
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
                if csi.final_byte == b'@' {
                    s.row().shift_right(col, p(0, 1), style);
                } else {
                    s.row().shift_left(col, p(0, 1), style);
                }
                s.wrap_pending = false;
            }
            b'L' | b'M' => {
                let s = self.screen_mut();
                if s.cursor.row >= s.top && s.cursor.row <= s.bottom {
                    let top = s.top;
                    s.top = s.cursor.row;
                    if csi.final_byte == b'L' {
                        s.scroll_down(p(0, 1), style);
                    } else {
                        s.scroll_up(p(0, 1), false, style);
                    }
                    s.top = top;
                    s.cursor.col = 0;
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
            b's' => self.save_cursor(),
            b'u' => self.restore_cursor(),
            b'g' => {
                let col = self.screen().cursor.col;
                match first {
                    0 => self.tabs[col] = false,
                    3 => self.tabs.fill(false),
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
                    let previous = (0..col).rev().find(|&c| self.tabs[c]).unwrap_or(0);
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
                        format!("\x1b[{prefix}{row};{}{page}R", s.cursor.col + 1).as_bytes(),
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
                    self.screen_mut().active_link = None;
                }
            } else {
                let id = if let Some(id) = id {
                    LinkIdentity::Explicit(id.into())
                } else {
                    let id = self.next_link_id;
                    self.next_link_id = self.next_link_id.wrapping_add(1);
                    LinkIdentity::Implicit(id)
                };
                self.screen_mut().active_link = Some(LinkRef::new(Hyperlink {
                    id,
                    uri: uri.into(),
                }));
            }
            return;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        let (command, data) = text.split_once(';').unwrap_or((text, ""));
        match command {
            "0" | "2" => self.event(TerminalEvent::Title(data.chars().take(4096).collect())),
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
            "52" => {
                if let Some((selection, data)) = data.split_once(';') {
                    // At most one large clipboard request survives a feed batch.
                    self.events
                        .retain(|e| !matches!(e, TerminalEvent::Clipboard { .. }));
                    self.event(TerminalEvent::Clipboard {
                        selection: selection.chars().take(16).collect(),
                        data: data.to_owned(),
                    });
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
        if header.private.is_some() || header.intermediates != b"$" || header.final_byte != b'q' {
            return;
        }
        let reply = match bytes {
            b"m" => Some(self.style.sgr()),
            b"r" => Some(format!(
                "{};{}r",
                self.screen().top + 1,
                self.screen().bottom + 1
            )),
            b" q" => Some(format!("{} q", self.cursor_style)),
            _ => None,
        };
        if let Some(reply) = reply {
            self.reply(format!("\x1bP1$r{reply}\x1b\\").as_bytes());
        } else {
            self.reply(b"\x1bP0$r\x1b\\");
        }
    }
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

fn char_width(ch: char) -> usize {
    if ch.is_ascii() {
        1
    } else if matches!(ch, '\u{1f1e6}'..='\u{1f1ff}') {
        2
    } else {
        ch.width().unwrap_or(0).min(2)
    }
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
