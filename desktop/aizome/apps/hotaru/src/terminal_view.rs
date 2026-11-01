//! One native widget per terminal, with cached text and damage by shared row.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Cursor as PointerCursor, Ime, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, StateView, Status,
    Widget,
};
use hotaru_core::input as vt;
use hotaru_core::{
    Color as TerminalColor, ContentEpoch, ContentPosition, ContentSnapshot, Rgb, Snapshot, TerminalColors, attrs,
};

use crate::app::Msg;
use crate::input;
use crate::settings::Bindings;

const PAD: f64 = 10.0;
const CURSOR_TIMER: u32 = 0;
/// While a selection drag is above or below the text, it scrolls this often.
const DRAG_TIMER: u32 = 1;
const DRAG_INTERVAL: Duration = Duration::from_millis(50);
const SCROLL_FRAME_BYTES: usize = 8 * 1024 * 1024;
const CURSOR_INTERVAL: Duration = Duration::from_millis(600);

#[derive(Default)]
struct Blink {
    hidden: bool,
    armed: bool,
}
impl Blink {
    // None: unchanged, Some(true): replace deadline, Some(false): cancel.
    fn sync(&mut self, eligible: bool, reset: bool) -> Option<bool> {
        if !eligible {
            self.hidden = false;
            return std::mem::take(&mut self.armed).then_some(false);
        }
        if reset || !self.armed {
            self.hidden = false;
            self.armed = true;
            return Some(true);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorPaint {
    Block,
    Hollow,
    Underline,
    Bar,
}
fn cursor_paint(cursor: hotaru_core::Cursor, focused: bool, preedit: bool, hidden: bool) -> Option<CursorPaint> {
    if !cursor.in_viewport {
        return None;
    }
    if preedit {
        return Some(CursorPaint::Block);
    }
    if !cursor.visible {
        return None;
    }
    if !focused {
        return Some(CursorPaint::Hollow);
    }
    if cursor.blinking && hidden {
        return None;
    }
    Some(match cursor.shape {
        hotaru_core::CursorShape::Block => CursorPaint::Block,
        hotaru_core::CursorShape::Underline => CursorPaint::Underline,
        hotaru_core::CursorShape::Bar => CursorPaint::Bar,
    })
}

// Aizome's established terminal palette, shared visually with etc/foot/foot.ini.
const ANSI_NIGHT: [u32; 16] = [
    0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x565f89, 0xff899d, 0x9fe044,
    0xfaba4a, 0x8db0ff, 0xc7a9ff, 0xa4daff, 0xc0caf5,
];
const ANSI_DAY: [u32; 16] = [
    0xe9e9ed, 0xc4314b, 0x4f7a28, 0x9a6a12, 0x2e5fd0, 0x7247c9, 0x0b6f9c, 0x50577a, 0x8990b3, 0xd63d58, 0x5c8a30,
    0xb07a16, 0x3a6ee0, 0x8152d8, 0x1080b0, 0x1a1b26,
];

fn rgb(color: Color) -> Rgb {
    Rgb((color.r * 255.0).round() as u8, (color.g * 255.0).round() as u8, (color.b * 255.0).round() as u8)
}

pub(crate) fn colors_for(theme: &Theme) -> TerminalColors {
    let mut colors =
        TerminalColors { foreground: rgb(theme.text()), background: rgb(theme.surface()), ..TerminalColors::default() };
    let ansi = if theme.is_dark() { ANSI_NIGHT } else { ANSI_DAY };
    for (target, color) in colors.palette.iter_mut().zip(ansi) {
        *target = Rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
    }
    colors
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CellPoint {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: CellPoint,
    pub end: CellPoint,
}

impl Selection {
    pub fn is_empty(self) -> bool {
        self.anchor == self.end
    }
    fn ordered(self) -> (CellPoint, CellPoint) {
        if (self.anchor.row, self.anchor.col) <= (self.end.row, self.end.col) {
            (self.anchor, self.end)
        } else {
            (self.end, self.anchor)
        }
    }
    pub fn text(self, snapshot: &Snapshot) -> String {
        let (start, end) = self.ordered();
        snapshot.selection_text((start.row, start.col), (end.row, end.col))
    }
    pub(crate) fn contains(self, row: usize, col: usize) -> bool {
        let (start, end) = self.ordered();
        (row, col) >= (start.row, start.col) && (row, col) < (end.row, end.col)
    }
}

/// A point in the pane's text. `row` counts from the first row of the
/// content epoch, so it names the same text while output and scrolling move it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextPoint {
    pub row: u64,
    pub col: usize,
}

impl TextPoint {
    /// The text at `cell` of a snapshot at `position`.
    pub fn at(cell: CellPoint, position: &ContentPosition) -> Self {
        Self { row: position.origin + position.first_row as u64 + cell.row as u64, col: cell.col }
    }
}

/// A selection held on the pane's text, not on the screen: new output and
/// scrolling move it with the text it covers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSelection {
    pub epoch: ContentEpoch,
    pub anchor: TextPoint,
    pub end: TextPoint,
}

impl TextSelection {
    /// `selection` as it shows on a snapshot at `position`.
    pub fn on(selection: Selection, position: &ContentPosition) -> Self {
        Self {
            epoch: position.epoch.clone(),
            anchor: TextPoint::at(selection.anchor, position),
            end: TextPoint::at(selection.end, position),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.end
    }
    fn ordered(&self) -> (TextPoint, TextPoint) {
        if self.anchor <= self.end { (self.anchor, self.end) } else { (self.end, self.anchor) }
    }
    /// The first text row `snapshot` shows.
    fn top(snapshot: &Snapshot) -> u64 {
        snapshot.position.origin + snapshot.position.first_row as u64
    }
    /// The part of the selection that `snapshot` shows, in its screen rows.
    /// None when none of it shows, or the snapshot shows other text.
    pub fn visible(&self, snapshot: &Snapshot) -> Option<Selection> {
        if self.epoch != snapshot.position.epoch || snapshot.rows == 0 {
            return None;
        }
        let top = Self::top(snapshot);
        let bottom = top + snapshot.rows as u64; // Exclusive.
        let (start, end) = self.ordered();
        let start = if start.row < top { TextPoint { row: top, col: 0 } } else { start };
        let end = if end.row >= bottom { TextPoint { row: bottom - 1, col: snapshot.cols } } else { end };
        if start >= end {
            return None;
        }
        let cell = |point: TextPoint| CellPoint { row: (point.row - top) as usize, col: point.col };
        Some(Selection { anchor: cell(start), end: cell(end) })
    }
    /// Whether `snapshot` shows all of the selection.
    pub fn shown_whole(&self, snapshot: &Snapshot) -> bool {
        let top = Self::top(snapshot);
        let (start, end) = self.ordered();
        self.epoch == snapshot.position.epoch && start.row >= top && end.row < top + snapshot.rows as u64
    }
    /// The selection's endpoints in a capture of all the content, where row
    /// zero is the oldest row the capture keeps. None when the capture is
    /// other text, or keeps none of the selection.
    pub fn in_content(&self, content: &ContentSnapshot) -> Option<((usize, usize), (usize, usize))> {
        if self.epoch != content.position.epoch || content.rows == 0 {
            return None;
        }
        let origin = content.position.origin;
        let last = origin + content.rows as u64 - 1;
        let (start, end) = self.ordered();
        if end.row < origin || start.row > last {
            return None;
        }
        let start = if start.row < origin { (0, 0) } else { ((start.row - origin) as usize, start.col) };
        let end =
            if end.row > last { (content.rows - 1, content.cols) } else { ((end.row - origin) as usize, end.col) };
        Some((start, end))
    }
    /// The selected text that `snapshot` shows.
    pub fn text(&self, snapshot: &Snapshot) -> String {
        self.visible(snapshot).map(|selection| selection.text(snapshot)).unwrap_or_default()
    }
}

#[allow(clippy::too_many_arguments)] // The terminal's immutable view inputs travel together.
pub fn terminal_view(
    id: u64,
    snapshot: Arc<Snapshot>,
    font: (f64, u16),
    selection: Option<&TextSelection>,
    bindings: Arc<Bindings>,
    rendered: Option<Arc<crate::font::Frame>>,
    input_activity: u64,
    automatic: Option<crate::links::Link>,
    menu: Menu<Msg>,
) -> Element<Msg> {
    let snapshot = rendered.as_ref().map_or(snapshot, |frame| frame.snapshot.clone());
    let selected = selection.is_some_and(|selection| !selection.is_empty());
    let selection = selection.and_then(|selection| selection.visible(&snapshot));
    let (font_size, font_weight) = font;
    Element::new(
        TerminalView {
            id,
            snapshot,
            font_size,
            font_weight,
            selection,
            selected,
            bindings,
            rendered,
            input_activity,
            automatic,
            menu,
        },
        vec![],
    )
}

struct TerminalView {
    id: u64,
    snapshot: Arc<Snapshot>,
    font_size: f64,
    font_weight: u16,
    /// The part of the pane's selection this snapshot shows.
    selection: Option<Selection>,
    /// Whether the pane has a selection, shown or not.
    selected: bool,
    bindings: Arc<Bindings>,
    rendered: Option<Arc<crate::font::Frame>>,
    input_activity: u64,
    automatic: Option<crate::links::Link>,
    /// What a right click shows, when the program isn't taking the mouse.
    menu: Menu<Msg>,
}

/// Ghostty's word boundaries (`selection-word-chars`), beside spaces.
const WORD_BOUNDARIES: &str = "\t'\"│`|:;,()[]{}<>$";

/// The word, run of spaces or boundary character at `cell`, as a double
/// click selects it.
fn word_at(snapshot: &Snapshot, cell: CellPoint) -> Selection {
    let row = cell.row.min(snapshot.lines.len().saturating_sub(1));
    let cells = snapshot.lines.get(row).map_or(&[][..], |line| line.cells());
    let cols = cells.len().min(snapshot.cols);
    if cols == 0 {
        return Selection { anchor: CellPoint { row, col: 0 }, end: CellPoint { row, col: 0 } };
    }
    let col = cell.col.min(cols - 1);
    // A wide character's second cell belongs with its first.
    let class = |i: usize| match cells[i].character() {
        _ if cells[i].width() == 0 => 2,
        c if c.is_whitespace() => 0,
        c if WORD_BOUNDARIES.contains(c) => 1,
        _ => 2,
    };
    let at = class(col);
    let (mut start, mut end) = (col, col + 1);
    if at != 1 {
        while start > 0 && class(start - 1) == at {
            start -= 1;
        }
        while end < cols && class(end) == at {
            end += 1;
        }
    }
    Selection { anchor: CellPoint { row, col: start }, end: CellPoint { row, col: end } }
}

/// The whole line at `row`, across soft wraps, as a triple click selects it.
fn line_at(snapshot: &Snapshot, row: usize) -> Selection {
    let last = snapshot.lines.len().saturating_sub(1);
    let (mut start, mut end) = (row.min(last), row.min(last));
    while start > 0 && snapshot.lines[start - 1].wrapped() {
        start -= 1;
    }
    while end < last && snapshot.lines[end].wrapped() {
        end += 1;
    }
    Selection { anchor: CellPoint { row: start, col: 0 }, end: CellPoint { row: end, col: snapshot.cols } }
}

#[derive(Default)]
struct ViewState {
    // Keep the preceding rows alive until layout has compared them. Raw address
    // caches can mistake allocator reuse for unchanged content between frames.
    snapshot: Option<Arc<Snapshot>>,
    rendered: Option<Arc<crate::font::Frame>>,
    cursor: Option<hotaru_core::Cursor>,
    blink: Blink,
    input_activity: u64,
    last_output_reset: Option<Instant>,
    size: Size,
    cell_width: f64,
    line_height: f64,
    padding: f64,
    scale: f64,
    scale_pending: bool,
    font_epoch: Option<u64>,
    font_metrics: Option<aizome_kit::sidestep_ui::grid::Metrics>,
    /// Where a selection drag began, on the text.
    drag: Option<TextPoint>,
    /// Where the pointer is during a selection drag.
    drag_pointer: Option<Point>,
    preedit: String,
    scroll_fraction: f64,
    resize_pending: bool,
    theme: Option<(bool, Rgb, Rgb)>,
    theme_pending: bool,
    window_rect: Rect,
    keys: Vec<(u32, vt::KeyEvent)>,
    buttons: Vec<vt::MouseButton>,
    hovered: Option<crate::links::Link>,
    auto_request: Option<Arc<crate::automatic_links::Request>>,
    auto_revision: Option<Arc<crate::automatic_links::Request>>,
    auto_refresh_pending: bool,
    link_press: Option<crate::links::Press>,
    pointer: Option<Point>,
    link_modifier: bool,
    hover_clear_pending: bool,
    sprite_overhang: bool,
}

fn retained_frame(frame: Option<&Arc<crate::font::Frame>>) -> Option<Arc<crate::font::Frame>> {
    frame.filter(|frame| frame.retention_bytes <= SCROLL_FRAME_BYTES).cloned()
}

// Position metadata proposes the usual viewport translation. Partial scroll
// regions keep that position unchanged, so owned row identities can propose
// one instead. In either case every retained destination needs both proofs;
// anything unproven is rasterized after the copy.
struct ScrollReuse {
    delta: isize,
    rows: std::ops::Range<usize>,
    clean: Vec<bool>,
}

fn scroll_reuse(old: &crate::font::Frame, new: &crate::font::Frame) -> Option<ScrollReuse> {
    if std::ptr::eq(old, new) {
        return None;
    }
    let (a, b) = (&old.snapshot, &new.snapshot);
    if old.sprite_overhang
        || new.sprite_overhang
        || old.retention_bytes > SCROLL_FRAME_BYTES
        || new.retention_bytes > SCROLL_FRAME_BYTES
        || a.position.epoch != b.position.epoch
        || a.cols != b.cols
        || a.rows != b.rows
        || old.epoch != new.epoch
        || old.metrics != new.metrics
        || a.palette != b.palette
        || a.default_foreground != b.default_foreground
        || a.default_background != b.default_background
        || old.rows.len() != a.rows
        || new.rows.len() != b.rows
        || a.lines.len() != a.rows
        || b.lines.len() != b.rows
    {
        return None;
    }
    let before = u128::from(a.position.origin) + a.position.first_row as u128;
    let after = u128::from(b.position.origin) + b.position.first_row as u128;
    let delta = i128::try_from(before).ok()? - i128::try_from(after).ok()?;
    let delta = isize::try_from(delta).ok()?;
    if old.rows.iter().chain(&new.rows).flat_map(|r| r.iter()).any(|s| s.selected || s.matched || s.current_match) {
        return None;
    }
    let inferred = delta == 0;
    let delta = if inferred { region_scroll_delta(old, new)? } else { delta };
    if delta.unsigned_abs() >= b.rows {
        return None;
    }
    let same = |source: usize, y: usize| {
        hotaru_core::RowRef::ptr_eq(&a.lines[source], &b.lines[y]) && Arc::ptr_eq(&old.rows[source], &new.rows[y])
    };
    let mut clean: Vec<_> = (0..b.rows)
        .map(|y| {
            let Some(source) = y.checked_add_signed(-delta).filter(|source| *source < a.rows) else {
                return false;
            };
            same(source, y)
        })
        .collect();
    let moved = clean.iter().filter(|clean| **clean).count();
    if moved < 2 {
        return None;
    }
    let mut rows = 0..b.rows;
    if inferred {
        // Moving a small scroll region must not dirty the rest of the screen.
        // Include both ends of every proved translation, then keep stationary
        // pixels outside this interval in place. Multiple sparse moving islands
        // can still span many unchanged rows: reject a raster-work regression.
        rows = b.rows..0;
        for (y, &reuse) in clean.iter().enumerate() {
            if reuse && !same(y, y) {
                let source = y.checked_add_signed(-delta)?;
                rows.start = rows.start.min(y.min(source));
                rows.end = rows.end.max(y.max(source) + 1);
            }
        }
        for y in rows.clone() {
            // The toolkit repaints exposed pixels. They cannot count toward
            // the copy's benefit merely because an identical shared row exists
            // elsewhere, outside the actual source rectangle.
            clean[y] &= y.checked_add_signed(-delta).is_some_and(|source| rows.contains(&source));
        }
        if clean[rows.clone()].iter().filter(|&&reuse| reuse).count() <= rows.clone().filter(|&y| same(y, y)).count() {
            return None;
        }
        for (y, reuse) in clean.iter_mut().enumerate() {
            if !rows.contains(&y) {
                *reuse = same(y, y);
            }
        }
    }
    Some(ScrollReuse { delta, rows, clean })
}

fn region_scroll_delta(old: &crate::font::Frame, new: &crate::font::Frame) -> Option<isize> {
    // Bound scratch work independently of the retained-frame byte limit. Most
    // ordinary edits change only one row and avoid allocating either table.
    let rows = new.snapshot.rows;
    if !(3..=4096).contains(&rows)
        || old
            .snapshot
            .lines
            .iter()
            .zip(&new.snapshot.lines)
            .filter(|(a, b)| !hotaru_core::RowRef::ptr_eq(a, b))
            .take(2)
            .count()
            < 2
    {
        return None;
    }
    let mut sources = std::collections::HashMap::new();
    sources.try_reserve(rows).ok()?;
    for (y, row) in old.snapshot.lines.iter().enumerate() {
        // Both snapshots own their rows throughout this lookup. Addresses are
        // identity keys only, never dereferenced, and cannot be recycled here.
        // Duplicate identities are ambiguous; omit them as translation hints.
        sources.entry(&**row as *const hotaru_core::Row).and_modify(|source| *source = None).or_insert(Some(y));
    }
    let mut votes = Vec::new();
    votes.try_reserve_exact(rows * 2 - 1).ok()?;
    votes.resize(rows * 2 - 1, 0usize);
    for (y, row) in new.snapshot.lines.iter().enumerate() {
        let Some(Some(source)) = sources.get(&(&**row as *const hotaru_core::Row)) else { continue };
        if y != *source && Arc::ptr_eq(&old.rows[*source], &new.rows[y]) {
            votes[y + rows - 1 - source] += 1;
        }
    }
    let mut best = None;
    let mut count = 1;
    for (index, &matches) in votes.iter().enumerate() {
        if matches > count {
            count = matches;
            best = Some(index as isize - (rows - 1) as isize);
        }
    }
    best
}
fn pixel_aligned(value: f64, scale: f64) -> bool {
    let physical = value * scale;
    physical.is_finite() && (physical - physical.round()).abs() < 1e-7
}

fn emit(cx: &mut EventCx, msg: Msg) {
    cx.emit(&MsgFn::new(msg));
}

impl TerminalView {
    fn has_sprite_overhang(&self) -> bool {
        self.rendered.as_ref().is_some_and(|f| f.sprite_overhang)
    }
    fn link_point(&self, pos: Point, state: &ViewState) -> Option<CellPoint> {
        if self.rendered.is_none() || !pos.x.is_finite() || !pos.y.is_finite() {
            return None;
        }
        let x = pos.x - state.padding;
        let y = pos.y - state.padding;
        if x < 0.0 || y < 0.0 || state.cell_width <= 0.0 || state.line_height <= 0.0 {
            return None;
        }
        let cell =
            CellPoint { row: (y / state.line_height).floor() as usize, col: (x / state.cell_width).floor() as usize };
        (cell.row < self.snapshot.rows && cell.col < self.snapshot.cols).then_some(cell)
    }
    fn hover(&self, cx: &mut EventCx, pos: Option<Point>, modifier: bool) {
        let st = cx.state::<ViewState>();
        let cell = pos.and_then(|pos| self.link_point(pos, st));
        let explicit = cell.and_then(|cell| crate::links::Link::at(&self.snapshot, cell));
        let wanted =
            cell.filter(|cell| self.snapshot.lines[cell.row].hyperlink(cell.col).is_none()).zip(self.rendered.as_ref());
        let request_change = match wanted {
            Some((cell, frame)) => {
                let point = hotaru_link_regex::CellPoint { row: cell.row as u64, col: cell.col as u16 };
                if st.auto_request.as_ref().is_some_and(|request| request.same_location(self.id, &self.snapshot, point))
                {
                    None
                } else {
                    let request = Arc::new(crate::automatic_links::Request {
                        pane: self.id,
                        snapshot: self.snapshot.clone(),
                        cell: point,
                        retained_bytes: frame.retention_bytes,
                    });
                    st.auto_request = Some(request.clone());
                    Some(Some(request))
                }
            }
            None => st.auto_request.take().map(|_| None),
        };
        let hit = explicit.or_else(|| {
            self.automatic
                .as_ref()
                .filter(|hit| {
                    Some(hit.cell) == cell
                        && hit.matches_snapshot(&self.snapshot)
                        && hit.request().zip(st.auto_request.as_ref()).is_some_and(|(a, b)| Arc::ptr_eq(a, b))
                })
                .cloned()
        });
        let changed = match (&st.hovered, &hit) {
            (None, None) => false,
            (Some(a), Some(b)) => !a.same(b),
            _ => true,
        };
        let paint = changed || st.link_modifier != modifier;
        st.pointer = pos;
        st.link_modifier = modifier;
        st.hovered = hit.clone();
        if let Some(press) = &mut st.link_press {
            press.cancelled |= !modifier;
        }
        cx.set_cursor(if modifier && hit.is_some() { PointerCursor::Pointer } else { PointerCursor::Text });
        if paint {
            cx.invalidate();
        }
        if let Some(request) = request_change {
            emit(cx, Msg::MatchLink(self.id, request));
        }
        if changed {
            emit(cx, Msg::HoverLink(self.id, hit));
        }
    }
    fn mouse_input(
        &self,
        state: &ViewState,
        pos: Point,
        mods: aizome_kit::sidestep_ui::Modifiers,
        action: vt::MouseAction,
        button: Option<vt::MouseButton>,
    ) -> input::MouseInput {
        input::MouseInput {
            event: vt::MouseEvent {
                action,
                button,
                modifiers: input::modifiers(mods),
                x: pos.x as f32,
                y: pos.y as f32,
            },
            width: state.size.width,
            height: state.size.height,
            cell_width: state.cell_width,
            cell_height: state.line_height,
            padding: state.padding,
            scale: state.font_metrics.map_or(state.scale, |metrics| metrics.scale),
            any_button: !state.buttons.is_empty(),
        }
    }
    fn point(&self, pos: Point, state: &ViewState) -> CellPoint {
        CellPoint {
            row: (((pos.y - state.padding).max(0.0) / state.line_height.max(1.0)) as usize)
                .min(self.snapshot.rows.saturating_sub(1)),
            col: (((pos.x - state.padding).max(0.0) / state.cell_width.max(1.0)).round() as usize)
                .min(self.snapshot.cols),
        }
    }
    /// A selection drag's end moves to `pos`, on the text this view shows.
    fn drag_to(&self, cx: &mut EventCx, pos: Point) {
        let st = cx.state::<ViewState>();
        let Some(anchor) = st.drag else { return };
        let end = TextPoint::at(self.point(pos, st), &self.snapshot.position);
        let selection = TextSelection { epoch: self.snapshot.position.epoch.clone(), anchor, end };
        emit(cx, Msg::Select(self.id, selection));
    }
    /// The lines a selection drag at `pos` scrolls: back into the history
    /// above the text, forward below it, none where it can't go further.
    fn drag_scroll(&self, state: &ViewState, pos: Point) -> i32 {
        let position = &self.snapshot.position;
        if pos.y < state.padding && position.first_row > 0 {
            1
        } else if pos.y >= state.size.height - state.padding && position.first_row < position.history_rows {
            -1
        } else {
            0
        }
    }
    /// The cell under `pos`, where `point` is the edge between cells nearest it.
    fn cell(&self, pos: Point, state: &ViewState) -> CellPoint {
        CellPoint {
            col: (((pos.x - state.padding).max(0.0) / state.cell_width.max(1.0)) as usize)
                .min(self.snapshot.cols.saturating_sub(1)),
            ..self.point(pos, state)
        }
    }
    fn blink_eligible(&self, focused: bool, state: &ViewState) -> bool {
        focused
            && self.rendered.is_some()
            && self.snapshot.cursor.visible
            && self.snapshot.cursor.in_viewport
            && self.snapshot.cursor.blinking
            && state.preedit.is_empty()
    }
    fn reset_blink(&self, cx: &mut EventCx) {
        let focused = cx.has_keyboard_focus();
        let state = cx.state::<ViewState>();
        let hidden = state.blink.hidden;
        let eligible = self.blink_eligible(focused, state);
        let timer = state.blink.sync(eligible, true);
        let rect = self.cursor_rect(state);
        if let Some(arm) = timer {
            if arm {
                cx.set_timer(CURSOR_TIMER, CURSOR_INTERVAL);
            } else {
                cx.cancel_timer(CURSOR_TIMER);
            }
        }
        if hidden {
            if self.has_sprite_overhang() {
                cx.invalidate();
            } else {
                cx.invalidate_rect(rect);
            }
        }
    }
    fn cursor_rect(&self, state: &ViewState) -> Rect {
        let c = &self.snapshot.cursor;
        let mut col = c.col;
        let mut width = 1;
        if let Some(row) = self.snapshot.lines.get(c.row) {
            if row.cells().get(col).is_some_and(|c| c.width() == 0) {
                col = col.saturating_sub(1);
            }
            width = row.cells().get(col).map_or(1, |c| c.width().max(1));
        }
        Rect::from_origin_size(
            (state.padding + col as f64 * state.cell_width, state.padding + c.row as f64 * state.line_height),
            (width as f64 * state.cell_width, state.line_height),
        )
    }
}

impl Widget for TerminalView {
    fn name(&self) -> &'static str {
        "hotaru-terminal"
    }
    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let scale = cx.scale();
        let pad = (PAD * scale).round() / scale;
        // Initial geometry only; real face metrics arrive from the worker.
        let (cell_width, line_height) = self
            .rendered
            .as_ref()
            .map_or((self.font_size * (96.0 / 72.0) * 0.6, self.font_size * (96.0 / 72.0) * 1.2), |frame| {
                (frame.metrics.cell_width(), frame.metrics.cell_height())
            });
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 80.0 * cell_width + pad * 2.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 24.0 * line_height + pad * 2.0 },
        ));
        let cursor = self.snapshot.cursor;
        let focused = cx.is_focused();
        let now = cx.now();
        let theme = (cx.theme().is_dark(), rgb(cx.theme().text()), rgb(cx.theme().surface()));
        let st = cx.state::<ViewState>();
        let old_rendered = std::mem::replace(&mut st.rendered, retained_frame(self.rendered.as_ref()));
        // Retention may reject a large frame, so remember overhang separately.
        let repair_overhang = st.sprite_overhang || self.has_sprite_overhang();
        st.sprite_overhang = self.has_sprite_overhang();
        let overlays = !st.preedit.is_empty() || st.hovered.is_some() || self.selection.is_some();
        let old_scale = st.scale;
        let metrics = self.rendered.as_ref().map(|frame| frame.metrics);
        st.resize_pending |= st.font_metrics != metrics;
        st.font_metrics = metrics;
        st.scale_pending |= st.scale != scale;
        st.scale = scale;
        st.padding = pad;
        let epoch = self.rendered.as_ref().map(|frame| frame.epoch);
        let font_changed = st.font_epoch != epoch;
        st.font_epoch = epoch;
        let resized = st.size != size || st.cell_width != cell_width || st.line_height != line_height;
        let revision = self.automatic.as_ref().and_then(|link| link.request()).cloned();
        let revision_changed = match (&st.auto_revision, &revision) {
            (None, None) => false,
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            _ => true,
        };
        st.auto_revision = revision;
        st.auto_refresh_pending |= revision_changed;
        let old_snapshot = st.snapshot.replace(self.snapshot.clone());
        if old_snapshot.as_ref().is_some_and(|old| !Arc::ptr_eq(old, &self.snapshot)) {
            st.hover_clear_pending |= st.hovered.take().is_some();
            // Continue only an automatic lookup. Explicit OSC8 hover keeps
            // its existing clear-until-pointer-move behavior on a new frame.
            st.auto_refresh_pending |= st.auto_request.is_some();
            st.auto_request = None;
            if let Some(press) = &mut st.link_press {
                press.cancelled = true;
            }
        }
        let old_cursor = st.cursor.replace(cursor);
        let input = st.input_activity != self.input_activity;
        st.input_activity = self.input_activity;
        let output = old_snapshot.as_ref().is_none_or(|old| old.generation != self.snapshot.generation)
            && st
                .last_output_reset
                .is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_millis(500));
        if output {
            st.last_output_reset = Some(now);
        }
        let eligible = self.blink_eligible(focused, st);
        let was_hidden = st.blink.hidden;
        let timer = st.blink.sync(eligible, input || output || old_cursor != Some(cursor));
        st.size = size;
        st.cell_width = cell_width;
        st.line_height = line_height;
        st.resize_pending |= resized;
        st.theme_pending |= st.theme.replace(theme) != Some(theme);
        let content = Rect::new(
            pad,
            pad,
            pad + self.snapshot.cols as f64 * cell_width,
            pad + self.snapshot.rows as f64 * line_height,
        );
        let reuse = if !repair_overhang
            && !resized
            && !font_changed
            && !overlays
            && !st.theme_pending
            && old_scale == scale
            && content.x1 <= size.width - pad + 1e-7
            && content.y1 <= size.height - pad + 1e-7
            && st.window_rect.size() == size
            && pixel_aligned(st.window_rect.x0 + pad, scale)
            && pixel_aligned(st.window_rect.y0 + pad, scale)
            && pixel_aligned(content.width(), scale)
            && pixel_aligned(content.height(), scale)
            && pixel_aligned(line_height, scale)
        {
            old_rendered.as_ref().zip(self.rendered.as_ref()).and_then(|(old, new)| scroll_reuse(old, new))
        } else {
            None
        };

        if st.resize_pending
            || old_cursor != Some(cursor)
            || st.theme_pending
            || st.scale_pending
            || st.hover_clear_pending
            || st.auto_refresh_pending
        {
            cx.request_tick(Duration::ZERO);
        }
        if let Some(arm) = timer {
            if arm {
                cx.set_timer(CURSOR_TIMER, CURSOR_INTERVAL);
            } else {
                cx.cancel_timer(CURSOR_TIMER);
            }
        }
        if repair_overhang {
            cx.invalidate_rect(Rect::from_origin_size(Point::ZERO, size));
        }
        if was_hidden && (input || output) {
            let rect = self.cursor_rect(cx.state::<ViewState>());
            if self.has_sprite_overhang() {
                cx.invalidate_rect(Rect::from_origin_size(Point::ZERO, size));
            } else {
                cx.invalidate_rect(rect);
            }
        }
        if let Some(reuse) = &reuse {
            let dy = reuse.delta as f64 * line_height;
            let mut fixed = Vec::with_capacity(3);
            // Repaint the old cursor AND where its old pixels were copied.
            // Full rows include wide tails, hollow borders and changed glyph
            // shaping boundaries. This is safe for every previous blink phase.
            for y in old_cursor.into_iter().map(|c| c.row).chain([cursor.row]) {
                if reuse.rows.contains(&y) {
                    fixed.push(Rect::new(
                        pad,
                        pad + y as f64 * line_height,
                        content.x1,
                        pad + (y + 1) as f64 * line_height,
                    ));
                }
            }
            if let Some(y) = old_cursor
                .filter(|c| reuse.rows.contains(&c.row))
                .and_then(|c| c.row.checked_add_signed(reuse.delta))
                .filter(|y| reuse.rows.contains(y))
            {
                fixed.push(Rect::new(
                    pad,
                    pad + y as f64 * line_height,
                    content.x1,
                    pad + (y + 1) as f64 * line_height,
                ));
            }
            let region = Rect::new(
                content.x0,
                pad + reuse.rows.start as f64 * line_height,
                content.x1,
                pad + reuse.rows.end as f64 * line_height,
            );
            cx.scrolled(region, Vec2::new(0.0, dy), &fixed);
        }
        for (y, row) in self.snapshot.lines.iter().enumerate() {
            let unchanged = reuse.as_ref().map_or_else(
                || {
                    old_snapshot
                        .as_ref()
                        .and_then(|s| s.lines.get(y))
                        .is_some_and(|old| hotaru_core::RowRef::ptr_eq(old, row))
                },
                |reuse| reuse.clean[y],
            );
            if font_changed || !unchanged {
                cx.invalidate_rect(Rect::new(
                    0.0,
                    pad + y as f64 * line_height,
                    size.width,
                    pad + (y + 1) as f64 * line_height,
                ));
            }
        }
        if old_cursor != Some(cursor) {
            for c in old_cursor.into_iter().chain([cursor]) {
                let y = c.row;
                cx.invalidate_rect(Rect::new(
                    0.0,
                    pad + y as f64 * line_height,
                    size.width,
                    pad + (y + 1) as f64 * line_height,
                ));
            }
        }
        size
    }
    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.font_size.to_bits());
        h.add(&self.font_weight);
        h.add(&self.selection);
        h.add(&self.snapshot.palette);
        h.add(&self.snapshot.default_foreground);
        h.add(&self.snapshot.default_background);
        h.add(&(self.snapshot.cols, self.snapshot.rows));
        h.add(&self.rendered.as_ref().map(|f| (f.epoch, f.metrics.scale.to_bits())));
    }
    fn paint(&self, cx: &mut PaintCx) {
        let Some(state) = cx.state::<ViewState>() else { return };
        let cw = state.cell_width;
        let lh = state.line_height;
        let pad = state.padding;
        let theme = cx.theme();
        let bounds = cx.bounds();
        let background = self.snapshot.default_background;
        cx.canvas.fill_rect(bounds, Color::rgb8(background.0, background.1, background.2));
        cx.canvas.save();
        cx.canvas.clip_rect(bounds);
        let overhang = self.has_sprite_overhang();
        let cursor_kind = cursor_paint(
            self.snapshot.cursor,
            cx.is_focused() && cx.window_active(),
            !state.preedit.is_empty(),
            state.blink.hidden,
        );
        let block_cursor = (overhang && cursor_kind == Some(CursorPaint::Block)).then(|| self.cursor_rect(state));

        // Normal masks stay within cells. Rare oversized ink must be drawn
        // after every background, so adjacent spans/rows cannot erase it.
        for pass in 0..if overhang { 2 } else { 1 } {
            if pass == 1
                && let Some(rect) = block_cursor
            {
                let value = self.snapshot.default_foreground;
                cx.canvas.fill_rect(rect, Color::rgb8(value.0, value.1, value.2));
            }
            for (y, spans) in self.rendered.iter().flat_map(|frame| frame.rows.iter().enumerate()) {
                let row_rect = Rect::new(pad, pad + y as f64 * lh, bounds.width() - pad, pad + (y + 1) as f64 * lh);
                if !overhang && !cx.canvas.needs(row_rect) {
                    continue;
                }
                for span in spans.iter() {
                    let style = span.style;
                    let color = |c: TerminalColor, foreground: bool| match c {
                        TerminalColor::Default => {
                            let c = if foreground {
                                self.snapshot.default_foreground
                            } else {
                                self.snapshot.default_background
                            };
                            Color::rgb8(c.0, c.1, c.2)
                        }
                        TerminalColor::Indexed(i) => {
                            let c = self.snapshot.palette[usize::from(i)];
                            Color::rgb8(c.0, c.1, c.2)
                        }
                        TerminalColor::Rgb(c) => Color::rgb8(c.0, c.1, c.2),
                    };
                    let mut fg = color(style.foreground, true);
                    let mut bg = color(style.background, false);
                    if style.attrs & attrs::INVERSE != 0 {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    if span.selected || span.current_match {
                        bg = theme.selection();
                        fg = theme.on_selection();
                    } else if span.matched {
                        bg = theme.focus().with_alpha(0.3);
                    }
                    if style.attrs & attrs::DIM != 0 {
                        fg = fg.with_alpha(0.6);
                    }
                    let r =
                        Rect::new(pad + span.start as f64 * cw, row_rect.y0, pad + span.end as f64 * cw, row_rect.y1);
                    if !overhang || pass == 0 {
                        cx.canvas.fill_rect(r, bg);
                    }
                    if overhang && pass == 0 {
                        continue;
                    }
                    if style.attrs & attrs::HIDDEN != 0 {
                        continue;
                    }
                    cx.canvas.save();
                    cx.canvas.clip_rect(r);
                    let cursor_row = block_cursor.filter(|cursor| cursor.y0 == r.y0);
                    let cursor_color = self.snapshot.default_background;
                    let cursor_color = Color::rgb8(cursor_color.0, cursor_color.1, cursor_color.2);
                    let underline_color = if style.underline_color == TerminalColor::Default {
                        fg
                    } else {
                        color(style.underline_color, true)
                    };
                    let decoration_colors = DecorationColors {
                        foreground: fg,
                        underline: underline_color,
                        cursor: cursor_row.map(|r| (r, cursor_color)),
                    };
                    decorations(cx.canvas, r, style, span.layout.metrics, decoration_colors, false);
                    if let Some(cursor) = cursor_row {
                        // Tint during the foreground pass; painting a late
                        // cursor background would erase neighboring overhang.
                        for (clip, color) in [
                            (Rect::new(r.x0, r.y0, cursor.x0.min(r.x1), r.y1), fg),
                            (r.intersect(cursor), cursor_color),
                            (Rect::new(cursor.x1.max(r.x0), r.y0, r.x1, r.y1), fg),
                        ] {
                            if clip.x1 > clip.x0 {
                                cx.canvas.save();
                                cx.canvas.clip_rect(clip);
                                cx.canvas.draw_grid_text(&span.layout, Point::new(pad, r.y0), color);
                                cx.canvas.restore();
                            }
                        }
                    } else {
                        cx.canvas.draw_grid_text(&span.layout, Point::new(pad, r.y0), fg);
                    }
                    let sprite_cursor = cursor_row.map(|c| ((c.x0 - pad) / cw, (c.x1 - pad) / cw, cursor_color));
                    if overhang {
                        cx.canvas.restore();
                        draw_sprites(cx.canvas, span, Point::new(pad, r.y0), fg, sprite_cursor);
                        cx.canvas.save();
                        cx.canvas.clip_rect(r);
                    } else {
                        draw_sprites(cx.canvas, span, Point::new(pad, r.y0), fg, sprite_cursor);
                    }
                    decorations(cx.canvas, r, style, span.layout.metrics, decoration_colors, true);
                    cx.canvas.restore();
                }
            }
        }
        if let Some(hovered) = &state.hovered
            && state.link_modifier
            && hovered.matches_snapshot(&self.snapshot)
            && let Some(frame) = &self.rendered
        {
            for row in 0..self.snapshot.rows {
                if let Some(span) = hovered.automatic_span(row, self.snapshot.cols) {
                    let r = Rect::new(
                        pad + span.start as f64 * cw,
                        pad + row as f64 * lh,
                        pad + span.end as f64 * cw,
                        pad + (row + 1) as f64 * lh,
                    );
                    underline(cx.canvas, r, hotaru_core::Underline::Single, theme.focus(), frame.metrics);
                }
            }
            for span in &frame.links {
                if self.snapshot.lines[span.row]
                    .hyperlink(span.start)
                    .is_some_and(|link| hovered.explicit_matches(link))
                {
                    let r = Rect::new(
                        pad + span.start as f64 * cw,
                        pad + span.row as f64 * lh,
                        pad + span.end as f64 * cw,
                        pad + (span.row + 1) as f64 * lh,
                    );
                    underline(cx.canvas, r, hotaru_core::Underline::Single, theme.focus(), frame.metrics);
                }
            }
        }
        if let Some(kind) = cursor_kind {
            let r = self.cursor_rect(state);
            let color = self.snapshot.default_foreground;
            let color = Color::rgb8(color.0, color.1, color.2);
            let stroke = (state.scale * 2.0).round().max(1.0) / state.scale;
            match kind {
                CursorPaint::Underline => cx.canvas.fill_rect(Rect::new(r.x0, r.y1 - stroke, r.x1, r.y1), color),
                CursorPaint::Bar => cx.canvas.fill_rect(Rect::new(r.x0, r.y0, r.x0 + stroke, r.y1), color),
                CursorPaint::Hollow => aizome_kit::draw::border(cx.canvas, r, color, 1.0 / state.scale),
                CursorPaint::Block if overhang => {}
                CursorPaint::Block => {
                    cx.canvas.fill_rect(r, color);
                    // Reuse shaped glyphs, clipped to the cursor's full cell(s),
                    // including a wide continuation. No shaping on the UI thread.
                    if let Some(spans) =
                        self.rendered.as_ref().and_then(|frame| frame.rows.get(self.snapshot.cursor.row))
                    {
                        cx.canvas.save();
                        cx.canvas.clip_rect(r);
                        for span in spans.iter() {
                            if pad + span.end as f64 * cw <= r.x0 || pad + span.start as f64 * cw >= r.x1 {
                                continue;
                            }
                            if span.style.attrs & attrs::HIDDEN != 0 {
                                continue;
                            }
                            let value = self.snapshot.default_background;
                            let cursor_foreground = Color::rgb8(value.0, value.1, value.2);
                            let decoration_colors = DecorationColors {
                                foreground: cursor_foreground,
                                underline: cursor_foreground,
                                cursor: None,
                            };
                            let span_rect =
                                Rect::new(pad + span.start as f64 * cw, r.y0, pad + span.end as f64 * cw, r.y1);
                            decorations(
                                cx.canvas,
                                span_rect,
                                span.style,
                                span.layout.metrics,
                                decoration_colors,
                                false,
                            );
                            cx.canvas.draw_grid_text(
                                &span.layout,
                                Point::new(pad, r.y0),
                                Color::rgb8(value.0, value.1, value.2),
                            );
                            if !overhang {
                                draw_sprites(
                                    cx.canvas,
                                    span,
                                    Point::new(pad, r.y0),
                                    Color::rgb8(value.0, value.1, value.2),
                                    None,
                                );
                            }
                            decorations(cx.canvas, span_rect, span.style, span.layout.metrics, decoration_colors, true);
                        }
                        cx.canvas.restore();
                    }
                }
            }
            if !state.preedit.is_empty() {
                let mut spec = TextSpec::new(FontSpec::mono(self.font_size * (96.0 / 72.0)), theme.text());
                spec.underline = true;
                let text = cx.layout_text(&state.preedit, &spec, None);
                cx.canvas.fill_rect(Rect::from_origin_size(r.origin(), (text.size().width, lh)), theme.sunk());
                cx.canvas.draw_text(&text, r.origin());
            }
        }
        cx.canvas.restore();
    }
    fn hit(&self) -> bool {
        true
    }
    fn focusable(&self) -> Focusable {
        Focusable::Click
    }
    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }
    fn cursor(&self) -> Option<PointerCursor> {
        Some(PointerCursor::Text)
    }
    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Timer(DRAG_TIMER) => {
                let st = cx.state::<ViewState>();
                let Some(pos) = st.drag.and(st.drag_pointer) else { return Status::Handled };
                // The end follows the pointer onto the text the last scroll showed.
                self.drag_to(cx, pos);
                let lines = self.drag_scroll(cx.state::<ViewState>(), pos);
                if lines != 0 {
                    emit(cx, Msg::Scroll(self.id, lines));
                    cx.set_timer(DRAG_TIMER, DRAG_INTERVAL);
                }
            }
            Event::Timer(CURSOR_TIMER) => {
                let focused = cx.has_keyboard_focus();
                let state = cx.state::<ViewState>();
                if self.blink_eligible(focused, state) && state.blink.armed {
                    state.blink.hidden = !state.blink.hidden;
                    let rect = self.cursor_rect(state);
                    if self.has_sprite_overhang() {
                        cx.invalidate();
                    } else {
                        cx.invalidate_rect(rect);
                    }
                    cx.set_timer(CURSOR_TIMER, CURSOR_INTERVAL);
                } else {
                    state.blink.sync(false, false);
                    cx.cancel_timer(CURSOR_TIMER);
                }
            }
            Event::Tick => {
                let st = cx.state::<ViewState>();
                let clear_hover = std::mem::take(&mut st.hover_clear_pending);
                let refresh_hover = std::mem::take(&mut st.auto_refresh_pending);
                let hover_position = st.pointer;
                let hover_modifier = st.link_modifier;
                let cols = ((st.size.width - st.padding * 2.0) / st.cell_width).floor().max(1.0) as usize;
                let rows = ((st.size.height - st.padding * 2.0) / st.line_height).floor().max(1.0) as usize;
                let resized = std::mem::take(&mut st.resize_pending);
                let scale_changed = std::mem::take(&mut st.scale_pending);
                let scale = st.scale;
                let theme_changed = std::mem::take(&mut st.theme_pending);
                let rect = self.cursor_rect(st);
                if clear_hover {
                    cx.set_cursor(PointerCursor::Text);
                    emit(cx, Msg::HoverLink(self.id, None));
                }
                if refresh_hover {
                    self.hover(cx, hover_position, hover_modifier);
                }
                let window_rect = cx.window_rect();
                if cx.state::<ViewState>().window_rect != window_rect {
                    cx.state::<ViewState>().window_rect = window_rect;
                    emit(cx, Msg::Geometry(self.id, window_rect));
                }
                if theme_changed {
                    let colors = Arc::new(colors_for(cx.theme()));
                    emit(cx, Msg::Theme(colors));
                }
                if resized {
                    let (width, height) = self.rendered.as_ref().map_or((0, 0), |frame| {
                        (
                            cols.saturating_mul(frame.metrics.cell_width_px as usize).min(u16::MAX as usize) as u16,
                            rows.saturating_mul(frame.metrics.cell_height_px as usize).min(u16::MAX as usize) as u16,
                        )
                    });
                    emit(cx, Msg::ResizePixels(self.id, cols, rows, width, height));
                }
                if scale_changed {
                    emit(cx, Msg::ShapeScale(self.id, scale));
                }
                if cx.is_focused() {
                    cx.set_text_input(true, Some(rect));
                }
            }
            Event::Key(key) => {
                if key.pressed {
                    self.reset_blink(cx);
                }
                let position = cx.state::<ViewState>().pointer;
                self.hover(cx, position, key.modifiers.super_key());
                if key.composing.as_ref().is_none_or(|text| text.is_empty())
                    && let Some(action) = self.bindings.action(key)
                {
                    emit(cx, Msg::Action(action));
                    return Status::Handled;
                }
                let normalized = input::key_event(key);
                let state = cx.state::<ViewState>();
                if key.pressed {
                    if !state.keys.iter().any(|(code, _)| *code == key.code) {
                        state.keys.push((key.code, normalized.clone()));
                    }
                } else if let Some(index) = state.keys.iter().position(|(code, _)| *code == key.code) {
                    state.keys.remove(index);
                } else {
                    return Status::Handled; // The native menu consumed its press, or another pane owned it.
                }
                emit(cx, Msg::Key(self.id, Arc::new(normalized)));
            }
            Event::Ime(Ime::Commit(text)) => {
                self.reset_blink(cx);
                emit(cx, Msg::Input(self.id, text.as_bytes().to_vec()));
            }
            Event::Ime(Ime::Preedit { text, .. }) => {
                cx.state::<ViewState>().preedit = text.clone();
                self.reset_blink(cx);
                cx.invalidate();
            }
            Event::PointerDown { pos, button, clicks, mods } => {
                self.hover(cx, Some(*pos), mods.super_key());
                // The program has the mouse while it tracks it; Shift takes
                // it back for the menu, the primary selection and selecting.
                let ours = mods.shift() || self.snapshot.modes.mouse_tracking == vt::MouseTracking::None;
                if ours && matches!(button, PointerButton::Right | PointerButton::Middle) {
                    cx.request_focus();
                    emit(cx, Msg::FocusPane(self.id));
                    if *button == PointerButton::Right {
                        cx.open_context_menu(self.menu.spec().clone(), *pos);
                    } else {
                        emit(cx, Msg::PastePrimary(self.id));
                    }
                    return Status::Handled;
                }
                if *button == PointerButton::Left
                    && mods.super_key()
                    && let Some(link) = cx.state::<ViewState>().hovered.clone()
                {
                    let st = cx.state::<ViewState>();
                    st.drag = None;
                    st.drag_pointer = None;
                    st.link_press = Some(crate::links::Press { link, position: (pos.x, pos.y), cancelled: false });
                    cx.request_focus();
                    cx.capture_pointer();
                    emit(cx, Msg::FocusPane(self.id));
                    return Status::Handled;
                }
                let st = cx.state::<ViewState>();
                let p = self.point(*pos, st);
                let mouse_button = input::mouse_button(*button);
                if let Some(button) = mouse_button
                    && !st.buttons.contains(&button)
                {
                    st.buttons.push(button);
                }
                let pointer = self.mouse_input(st, *pos, *mods, vt::MouseAction::Press, mouse_button);
                let select = *button == PointerButton::Left && ours;
                // A double click takes the word, a triple click the line.
                let picked = match clicks {
                    2 if select => Some(word_at(&self.snapshot, self.cell(*pos, st))),
                    3.. if select => Some(line_at(&self.snapshot, p.row)),
                    _ => None,
                };
                if select && picked.is_none() {
                    st.drag = Some(TextPoint::at(p, &self.snapshot.position));
                    st.drag_pointer = Some(*pos);
                }
                cx.request_focus();
                cx.capture_pointer();
                emit(cx, Msg::FocusPane(self.id));
                if !mods.shift() {
                    emit(cx, Msg::Mouse(self.id, pointer));
                }
                let position = &self.snapshot.position;
                if let Some(selection) = picked {
                    emit(cx, Msg::Select(self.id, TextSelection::on(selection, position)));
                    emit(cx, Msg::Selected(self.id));
                } else if select {
                    emit(cx, Msg::Select(self.id, TextSelection::on(Selection { anchor: p, end: p }, position)));
                }
            }
            Event::PointerMove { pos, mods, .. } => {
                self.hover(cx, Some(*pos), mods.super_key());
                let st = cx.state::<ViewState>();
                if let Some(press) = &mut st.link_press {
                    press.moved((pos.x, pos.y));
                    return Status::Handled;
                }
                let pointer = self.mouse_input(st, *pos, *mods, vt::MouseAction::Motion, st.buttons.last().copied());
                let dragging = st.drag.is_some();
                if dragging {
                    st.drag_pointer = Some(*pos);
                }
                if !mods.shift() {
                    emit(cx, Msg::Mouse(self.id, pointer));
                }
                if dragging {
                    self.drag_to(cx, *pos);
                    // Past the top or bottom of the text, the drag scrolls.
                    if self.drag_scroll(cx.state::<ViewState>(), *pos) != 0 {
                        cx.set_timer(DRAG_TIMER, DRAG_INTERVAL);
                    }
                }
            }
            Event::PointerUp { pos, button, mods, .. } => {
                self.hover(cx, Some(*pos), mods.super_key());
                if *button == PointerButton::Left
                    && let Some(press) = cx.state::<ViewState>().link_press.take()
                {
                    let hit = cx.state::<ViewState>().hovered.clone();
                    if let Some(link) = press.release(hit.as_ref(), (pos.x, pos.y), mods.super_key()) {
                        emit(cx, Msg::OpenLink(self.id, link));
                    }
                    cx.release_pointer();
                    return Status::Handled;
                }
                let st = cx.state::<ViewState>();
                let button = input::mouse_button(*button);
                st.buttons.retain(|held| Some(*held) != button);
                let dragged = st.drag.take().is_some();
                st.drag_pointer = None;
                let pointer = self.mouse_input(st, *pos, *mods, vt::MouseAction::Release, button);
                cx.cancel_timer(DRAG_TIMER);
                if !mods.shift() {
                    emit(cx, Msg::Mouse(self.id, pointer));
                }
                if dragged {
                    emit(cx, Msg::Selected(self.id));
                }
                cx.release_pointer();
            }
            Event::PointerLeave => {
                if let Some(press) = &mut cx.state::<ViewState>().link_press {
                    press.cancelled = true;
                }
                self.hover(cx, None, false);
            }
            Event::Scroll { pos, delta, mods, .. } => {
                if let Some(press) = &mut cx.state::<ViewState>().link_press {
                    press.cancelled = true;
                }
                let st = cx.state::<ViewState>();
                st.scroll_fraction -= delta.y / st.line_height.max(1.0);
                let lines = st.scroll_fraction.trunc() as i32;
                st.scroll_fraction -= f64::from(lines);
                let pointer = self.mouse_input(st, *pos, *mods, vt::MouseAction::Press, None);
                if lines != 0 && st.drag.is_some() {
                    // A selection drag's end follows the text the wheel shows.
                    cx.set_timer(DRAG_TIMER, DRAG_INTERVAL);
                }
                if lines != 0 {
                    if mods.shift() {
                        emit(cx, Msg::Scroll(self.id, lines));
                    } else {
                        emit(cx, Msg::Wheel(self.id, pointer, lines));
                    }
                }
            }
            Event::Focus(focused) | Event::WindowFocus(focused) => {
                self.reset_blink(cx);
                let rect = self.cursor_rect(cx.state::<ViewState>());
                cx.set_text_input(*focused, Some(rect));
                cx.invalidate();
                if *focused && matches!(event, Event::Focus(_)) {
                    emit(cx, Msg::FocusPane(self.id));
                }
                if !focused {
                    if let Some(press) = &mut cx.state::<ViewState>().link_press {
                        press.cancelled = true;
                    }
                    self.hover(cx, None, false);
                    for (_, mut key) in std::mem::take(&mut cx.state::<ViewState>().keys) {
                        key.action = vt::KeyAction::Release;
                        emit(cx, Msg::Key(self.id, Arc::new(key)));
                    }
                }
                emit(cx, Msg::TerminalFocus(self.id, *focused));
            }
            // Files and text dropped here paste; say so while they're over it.
            Event::DragOver { urls, has_text, .. } if !urls.is_empty() || *has_text => {}
            Event::Drop { urls, .. } if !urls.is_empty() => {
                emit(cx, Msg::PasteText(self.id, Arc::from(crate::selections::dropped(urls))));
            }
            Event::Drop { text: Some(text), .. } => {
                if text.len() > 32 * 1024 * 1024 - 12 {
                    emit(cx, Msg::Notice(self.id, "Dropped text exceeds the 32 MiB input limit.".into()));
                } else {
                    emit(cx, Msg::PasteText(self.id, Arc::from(text.as_str())));
                }
            }
            _ => return Status::Ignored,
        }
        Status::Handled
    }
    fn can(&self, _state: StateView, command: EditCommand) -> bool {
        match command {
            EditCommand::Copy => self.selected,
            EditCommand::Paste | EditCommand::Find => true,
            _ => false,
        }
    }
    /// The focused pane's edit commands: its window has the keyboard, so
    /// they're the front window's, as the menus' are.
    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        use crate::settings::Action;
        match command {
            EditCommand::Copy => emit(cx, Msg::Action(Action::Copy)),
            EditCommand::Paste => emit(cx, Msg::Action(Action::Paste)),
            EditCommand::Find => emit(cx, Msg::Action(Action::Find)),
            _ => return false,
        }
        true
    }
    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::MultilineTextInput, label: Some("Terminal".into()), ..Default::default() })
    }
}

#[derive(Clone, Copy)]
struct DecorationColors {
    foreground: Color,
    underline: Color,
    cursor: Option<(Rect, Color)>,
}
// Ghostty's text vertex shader applies cursor text color to all non-cursor
// source-cell quads, including decorations. Preserve the original span geometry
// while clipping color segments so dotted/dashed/curly phases do not restart.
fn decorations(
    canvas: &mut aizome_kit::sidestep_ui::Canvas,
    r: Rect,
    style: hotaru_core::Style,
    m: aizome_kit::sidestep_ui::grid::Metrics,
    colors: DecorationColors,
    above: bool,
) {
    if (above && style.attrs & attrs::STRIKE == 0)
        || (!above && style.attrs & attrs::OVERLINE == 0 && style.underline() == hotaru_core::Underline::None)
    {
        return;
    }
    let segments = if let Some((cursor, color)) = colors.cursor {
        [
            (Rect::new(r.x0, r.y0, cursor.x0.min(r.x1), r.y1), colors.foreground, colors.underline),
            (r.intersect(cursor), color, color),
            (Rect::new(cursor.x1.max(r.x0), r.y0, r.x1, r.y1), colors.foreground, colors.underline),
        ]
    } else {
        [(r, colors.foreground, colors.underline); 3]
    };
    for (clip, fg, ul) in segments.into_iter().take(if colors.cursor.is_some() { 3 } else { 1 }) {
        if clip.x1 <= clip.x0 {
            continue;
        }
        canvas.save();
        canvas.clip_rect(clip);
        if above {
            let y = r.y0 + f64::from(m.strike_y_px) / m.scale;
            canvas.fill_rect(Rect::new(r.x0, y, r.x1, y + f64::from(m.strike_thickness_px) / m.scale), fg);
        } else {
            underline(canvas, r, style.underline(), ul, m);
            if style.attrs & attrs::OVERLINE != 0 {
                canvas.fill_rect(Rect::new(r.x0, r.y0 + 1., r.x1, r.y0 + 2.), fg);
            }
        }
        canvas.restore();
    }
}

fn underline(
    canvas: &mut aizome_kit::sidestep_ui::Canvas,
    rect: Rect,
    kind: hotaru_core::Underline,
    color: Color,
    metrics: aizome_kit::sidestep_ui::grid::Metrics,
) {
    use hotaru_core::Underline;
    let thickness = f64::from(metrics.underline_thickness_px) / metrics.scale;
    let y = (rect.y0 + f64::from(metrics.underline_y_px) / metrics.scale).min(rect.y1 - thickness);
    match kind {
        Underline::None => {}
        Underline::Single => canvas.fill_rect(Rect::new(rect.x0, y, rect.x1, y + thickness), color),
        Underline::Double => {
            canvas.fill_rect(Rect::new(rect.x0, y, rect.x1, y + thickness), color);
            canvas.fill_rect(Rect::new(rect.x0, y - 2.0 * thickness, rect.x1, y - thickness), color);
        }
        Underline::Dotted | Underline::Dashed => {
            let length = thickness * if kind == Underline::Dotted { 1.0 } else { 4.0 };
            let mut x = rect.x0;
            while x < rect.x1 {
                canvas.fill_rect(Rect::new(x, y, (x + length).min(rect.x1), y + thickness), color);
                x += length + 2.0 * thickness;
            }
        }
        Underline::Curly => {
            let mut path = aizome_kit::sidestep_ui::kurbo::BezPath::new();
            path.move_to((rect.x0, y));
            let mut x = rect.x0;
            while x < rect.x1 {
                path.quad_to((x + 1.5, y - 2.0), ((x + 3.0).min(rect.x1), y));
                path.quad_to((x + 4.5, y + 2.0), ((x + 6.0).min(rect.x1), y));
                x += 6.0;
            }
            canvas.stroke(&path, &aizome_kit::sidestep_ui::StrokeStyle::new(thickness), color);
        }
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;
    #[test]
    fn cursor_priority_matches_ghostty_renderer() {
        for shape in
            [hotaru_core::CursorShape::Block, hotaru_core::CursorShape::Underline, hotaru_core::CursorShape::Bar]
        {
            for focused in [false, true] {
                for visible in [false, true] {
                    for blinking in [false, true] {
                        for hidden in [false, true] {
                            let mut c = hotaru_core::Cursor { shape, visible, blinking, ..Default::default() };
                            assert_eq!(cursor_paint(c, focused, true, hidden), Some(CursorPaint::Block));
                            let p = cursor_paint(c, focused, false, hidden);
                            if !visible || (focused && blinking && hidden) {
                                assert_eq!(p, None);
                            } else if !focused {
                                assert_eq!(p, Some(CursorPaint::Hollow));
                            } else {
                                assert_eq!(
                                    p,
                                    Some(match shape {
                                        hotaru_core::CursorShape::Block => CursorPaint::Block,
                                        hotaru_core::CursorShape::Underline => CursorPaint::Underline,
                                        hotaru_core::CursorShape::Bar => CursorPaint::Bar,
                                    })
                                );
                            }
                            c.in_viewport = false;
                            assert_eq!(cursor_paint(c, focused, true, hidden), None);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod scroll_retention_tests {
    use super::*;
    use crate::app::Hotaru;
    use aizome_kit::testing::Harness;
    use std::cell::Cell;

    struct HiddenFixture {
        app: Hotaru,
        window: Cell<Option<WindowKey>>,
    }
    impl Program for HiddenFixture {
        type Msg = Msg;
        fn info(&self) -> AppInfo {
            self.app.info()
        }
        fn launched(&mut self, cx: &mut Cx<Msg>) {
            self.app.launched(cx);
        }
        fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
            match msg {
                Msg::Notice(_, ref text) if text == "fixture-hide" => cx.hide_window(self.window.get().unwrap()),
                Msg::Notice(_, ref text) if text == "fixture-show" => cx.show_window(self.window.get().unwrap()),
                other => self.app.update(other, cx),
            }
        }
        fn view(&self, window: WindowKey) -> Element<Msg> {
            self.window.set(Some(window));
            self.app.view(window)
        }
    }
    fn frame(charge: usize) -> Arc<crate::font::Frame> {
        let fonts = aizome_kit::sidestep_ui::grid::FontSet::new(Default::default()).unwrap();
        Arc::new(crate::font::Frame {
            retention_bytes: charge,
            sprite_overhang: false,
            epoch: 0,
            snapshot: Arc::new(hotaru_core::Terminal::new(2, 2, 0).snapshot()),
            selection: None,
            metrics: fonts.metrics(1.).unwrap(),
            rows: Vec::new(),
            primary_family: String::new(),
            missing_families: Vec::new(),
            links: Vec::new(),
        })
    }
    fn identified_frame(rows: usize) -> Arc<crate::font::Frame> {
        let mut result = frame(1024);
        let f = Arc::get_mut(&mut result).unwrap();
        let mut terminal = hotaru_core::Terminal::new(4, rows, 0);
        for row in 1..=rows {
            terminal.feed(format!("\x1b[{row};1H{row}").as_bytes());
        }
        f.snapshot = Arc::new(terminal.snapshot());
        f.rows = (0..rows).map(|_| Arc::from(Vec::new())).collect();
        result
    }
    #[test]
    fn region_scroll_requires_unique_owned_and_shaped_identities() {
        let mut old = identified_frame(8);
        let mut new = frame(1024);
        let f = Arc::get_mut(&mut new).unwrap();
        f.snapshot = old.snapshot.clone();
        f.metrics = old.metrics;
        f.rows = old.rows.clone();
        assert!(scroll_reuse(&old, &new).is_none(), "unchanged rows do not propose a move");
        let f = Arc::get_mut(&mut new).unwrap();
        Arc::make_mut(&mut f.snapshot).lines[1..7].rotate_left(1);
        f.rows[1..7].rotate_left(1);
        let reuse = scroll_reuse(&old, &new).unwrap();
        assert_eq!(reuse.delta, -1);
        assert_eq!(reuse.rows, 1..7);
        assert_eq!(reuse.clean, [true, true, true, true, true, true, false, true]);
        let f = Arc::get_mut(&mut new).unwrap();
        f.rows = (0..8).map(|_| Arc::from(Vec::new())).collect();
        assert!(scroll_reuse(&old, &new).is_none(), "row identity alone cannot prove pixels");
        let f = Arc::get_mut(&mut old).unwrap();
        let snapshot = Arc::make_mut(&mut f.snapshot);
        let shared = snapshot.lines[0].clone();
        snapshot.lines.fill(shared);
        let f = Arc::get_mut(&mut new).unwrap();
        f.snapshot = old.snapshot.clone();
        Arc::make_mut(&mut f.snapshot).lines[0] = hotaru_core::Terminal::new(2, 1, 0).snapshot().lines[0].clone();
        Arc::make_mut(&mut f.snapshot).lines[7] = hotaru_core::Terminal::new(2, 1, 0).snapshot().lines[0].clone();
        f.rows = old.rows.clone();
        assert!(scroll_reuse(&old, &new).is_none(), "ambiguous repeated identities are not hints");
        let f = Arc::get_mut(&mut new).unwrap();
        Arc::make_mut(&mut f.snapshot).rows = 4097;
        assert!(region_scroll_delta(&old, &new).is_none(), "scratch inference is bounded");
    }
    #[test]
    fn region_scroll_keeps_stationary_pixels_and_rejects_sparse_copy_regressions() {
        let old = identified_frame(20);
        let mut new = frame(1024);
        let f = Arc::get_mut(&mut new).unwrap();
        f.metrics = old.metrics;
        f.snapshot = old.snapshot.clone();
        f.rows = old.rows.clone();
        Arc::make_mut(&mut f.snapshot).lines[5..9].rotate_left(1);
        f.rows[5..9].rotate_left(1);
        let reuse = scroll_reuse(&old, &new).unwrap();
        assert_eq!(reuse.rows, 5..9);
        assert_eq!(reuse.clean.iter().filter(|&&clean| clean).count(), 19);
        let f = Arc::get_mut(&mut new).unwrap();
        f.snapshot = old.snapshot.clone();
        f.rows = old.rows.clone();
        for rows in [2..5, 15..18] {
            Arc::make_mut(&mut f.snapshot).lines[rows.clone()].rotate_left(1);
            f.rows[rows].rotate_left(1);
        }
        assert!(scroll_reuse(&old, &new).is_none(), "four reused rows cannot justify dirtying ten stationary rows");
    }
    #[test]
    fn shared_stationary_rows_do_not_expand_the_copy_interval() {
        let mut old = identified_frame(20);
        let f = Arc::get_mut(&mut old).unwrap();
        let shared_row = f.snapshot.lines[0].clone();
        let shared_shape = f.rows[0].clone();
        for y in (0..5).chain(9..20) {
            Arc::make_mut(&mut f.snapshot).lines[y] = shared_row.clone();
            f.rows[y] = shared_shape.clone();
        }
        let mut new = frame(1024);
        let f = Arc::get_mut(&mut new).unwrap();
        f.metrics = old.metrics;
        f.snapshot = old.snapshot.clone();
        f.rows = old.rows.clone();
        Arc::make_mut(&mut f.snapshot).lines[5..9].rotate_left(1);
        f.rows[5..9].rotate_left(1);
        let reuse = scroll_reuse(&old, &new).unwrap();
        assert_eq!(reuse.rows, 5..9);
        assert_eq!(reuse.clean.iter().filter(|&&clean| clean).count(), 19);
    }
    #[test]
    fn exposed_rows_do_not_count_as_retained_pixels_in_the_work_gate() {
        let mut old = identified_frame(100);
        let f = Arc::get_mut(&mut old).unwrap();
        let shared_row = f.snapshot.lines[2].clone();
        let shared_shape = f.rows[2].clone();
        for y in (0..30).chain(32..100) {
            Arc::make_mut(&mut f.snapshot).lines[y] = shared_row.clone();
            f.rows[y] = shared_shape.clone();
        }
        let mut new = frame(1024);
        let f = Arc::get_mut(&mut new).unwrap();
        f.metrics = old.metrics;
        f.snapshot = old.snapshot.clone();
        f.rows = old.rows.clone();
        for (source, destination) in [(30, 0), (31, 1)] {
            Arc::make_mut(&mut f.snapshot).lines[destination] = old.snapshot.lines[source].clone();
            f.rows[destination] = old.rows[source].clone();
            Arc::make_mut(&mut f.snapshot).lines[source] = shared_row.clone();
            f.rows[source] = shared_shape.clone();
        }
        assert_eq!(region_scroll_delta(&old, &new), Some(-30));
        assert!(scroll_reuse(&old, &new).is_none(), "exposed shared rows cannot justify copying a large interval");
    }
    #[test]
    fn prior_frame_admission_is_bounded_and_preserves_snapshot_identity() {
        let normal = frame(SCROLL_FRAME_BYTES);
        let retained = retained_frame(Some(&normal)).unwrap();
        assert!(Arc::ptr_eq(&normal, &retained));
        assert!(Arc::ptr_eq(&normal.snapshot, &retained.snapshot));
        assert_eq!(normal.snapshot.cursor, retained.snapshot.cursor);
        assert!(retained_frame(Some(&frame(SCROLL_FRAME_BYTES + 1))).is_none());
        assert!(retained_frame(Some(&frame(usize::MAX))).is_none());
        let weak = Arc::downgrade(&normal);
        let mut state = ViewState { rendered: Some(retained), ..Default::default() };
        drop(normal);
        assert!(weak.upgrade().is_some());
        state.rendered = retained_frame(Some(&frame(usize::MAX)));
        assert!(weak.upgrade().is_none(), "oversize replacement drops previous owned frame");
        state.rendered = retained_frame(Some(&frame(1024)));
        let weak = Arc::downgrade(state.rendered.as_ref().unwrap());
        drop(state);
        assert!(weak.upgrade().is_none(), "widget teardown releases retained frame");
    }

    #[test]
    fn hidden_window_retains_only_its_last_admitted_view_and_releases_it_on_show() {
        let mut h = Harness::new(HiddenFixture { app: Hotaru::preview(), window: Cell::new(None) });
        let window = h.window();
        assert!(h.settle(Duration::from_secs(5), |app| app.app.active_pane().unwrap().rendered.is_some()));
        h.advance(Duration::ZERO);
        let pane = h.program().app.active_pane().unwrap();
        let id = pane.id;
        let original = Arc::downgrade(pane.rendered.as_ref().unwrap());
        assert!(original.upgrade().unwrap().retention_bytes <= SCROLL_FRAME_BYTES);
        h.send(Msg::Notice(id, "fixture-hide".into()));
        assert!(!h.is_shown(window));
        let mut terminal = hotaru_core::Terminal::new(70, 16, 64);
        let mut frames = Vec::new();
        for n in 0..12 {
            terminal.feed(format!("hidden output {n}\r\n").as_bytes());
            h.send(Msg::Frame(id, Arc::new(terminal.snapshot())));
            assert!(h.settle(Duration::from_secs(5), |app| {
                let pane = app.app.active_pane().unwrap();
                pane.rendered.as_ref().is_some_and(|frame| Arc::ptr_eq(&frame.snapshot, &pane.snapshot))
            }));
            frames.push(Arc::downgrade(h.program().app.active_pane().unwrap().rendered.as_ref().unwrap()));
        }
        assert!(original.upgrade().is_some(), "hidden tree retains only the last displayed frame");
        assert!(frames[..frames.len() - 1].iter().all(|frame| frame.upgrade().is_none()));
        h.send(Msg::Notice(id, "fixture-show".into()));
        h.advance(Duration::ZERO);
        assert!(h.is_shown(window));
        assert!(original.upgrade().is_none(), "show replaces the old retained view");
        h.send(Msg::CloseTab(window));
        h.advance(Duration::ZERO);
        assert!(frames.iter().all(|frame| frame.upgrade().is_none()), "closing releases the final displayed frame");
    }
}

fn draw_sprites(
    canvas: &mut aizome_kit::sidestep_ui::Canvas,
    span: &crate::font::Span,
    origin: Point,
    color: Color,
    cursor: Option<(f64, f64, Color)>,
) {
    let metrics = span.layout.metrics;
    for sprite in span.sprites.iter() {
        let color = cursor
            .filter(|(start, end, _)| (sprite.column as f64) < *end && (sprite.column + sprite.width) as f64 > *start)
            .map_or(color, |(_, _, color)| color);
        debug_assert!(sprite.column >= span.start && sprite.column + sprite.width <= span.end);
        let placed = Point::new(
            origin.x + sprite.column as f64 * metrics.cell_width() + f64::from(sprite.glyph.origin_x) / metrics.scale,
            origin.y + f64::from(sprite.glyph.origin_y) / metrics.scale,
        );
        assert!(canvas.draw_grid_mask(&sprite.glyph.mask, placed, metrics.scale, color), "validated sprite geometry");
    }
}

#[cfg(test)]
mod sprite_paint_tests {
    use super::*;
    use aizome_kit::sidestep_ui::grid;
    use aizome_kit::testing::Harness;
    struct Fixture {
        frame: Arc<crate::font::Frame>,
    }
    impl Program for Fixture {
        type Msg = Msg;
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.SpriteTest", "Sprites", "Sprites")
        }
        fn launched(&mut self, cx: &mut Cx<Msg>) {
            cx.open_window(WindowSpec::new("Sprites").size(96.0, 96.0));
        }
        fn update(&mut self, msg: Msg, _: &mut Cx<Msg>) {
            if let Msg::Shaped(_, frame) = msg {
                self.frame = frame;
            }
        }
        fn view(&self, _: WindowKey) -> Element<Msg> {
            terminal_view(
                1,
                self.frame.snapshot.clone(),
                (14.0, 400),
                None,
                Arc::new(Bindings::new(&crate::settings::Settings::default())),
                Some(self.frame.clone()),
                0,
                None,
                Menu::new("Terminal"),
            )
            .id("grid")
        }
    }
    fn frame(sprite: bool) -> Arc<crate::font::Frame> {
        let mut terminal = hotaru_core::Terminal::new(4, 4, 0);
        terminal.feed(b"\x1b[?25l\x1b[48;2;0;0;200m\x1b[2J");
        let fonts = grid::FontSet::new(Default::default()).unwrap();
        let mut metrics = fonts.metrics(1.0).unwrap();
        metrics.cell_width_px = 8;
        metrics.cell_height_px = 12;
        metrics.box_thickness_px = 8;
        let layout = Arc::new(grid::Layout { metrics, runs: Arc::from([]), clusters: Arc::from([]) });
        let mut cache = crate::sprites::Cache::default();
        let glyph = cache.get(0x2501, metrics).unwrap();
        assert!(glyph.overhang);
        let mut rows = Vec::new();
        for row in 0..4 {
            let sprites = if sprite && row == 1 {
                vec![crate::sprites::Instance { column: 1, width: 1, glyph: glyph.clone() }]
            } else {
                Vec::new()
            };
            rows.push(Arc::from(vec![crate::font::Span {
                start: 0,
                end: 4,
                style: hotaru_core::Style {
                    foreground: TerminalColor::Rgb(Rgb(255, 255, 255)),
                    background: TerminalColor::Rgb(Rgb(0, 0, 200)),
                    ..Default::default()
                },
                selected: false,
                matched: false,
                current_match: false,
                layout: layout.clone(),
                sprites: Arc::from(sprites),
            }]));
        }
        Arc::new(crate::font::Frame {
            retention_bytes: usize::MAX,
            sprite_overhang: sprite,
            epoch: 1,
            snapshot: Arc::new(terminal.snapshot()),
            selection: None,
            metrics,
            rows,
            primary_family: String::new(),
            missing_families: Vec::new(),
            links: Vec::new(),
        })
    }
    #[test]
    fn block_cursor_keeps_and_tints_decorations_in_both_paint_paths() {
        for kind in [
            hotaru_core::Underline::Single,
            hotaru_core::Underline::Double,
            hotaru_core::Underline::Dotted,
            hotaru_core::Underline::Dashed,
            hotaru_core::Underline::Curly,
        ] {
            for wide in [false, true] {
                let mut images = Vec::new();
                for overhang in [false, true] {
                    let mut owned = frame(false);
                    let f = Arc::get_mut(&mut owned).unwrap();
                    f.sprite_overhang = overhang;
                    let mut term = hotaru_core::Terminal::new(4, 4, 0);
                    if wide {
                        term.feed("\x1b[1;2H界\x1b[1;2H".as_bytes());
                    } else {
                        term.feed(b"\x1b[1;2H");
                    }
                    f.snapshot = Arc::new(term.snapshot());
                    let snap = Arc::get_mut(&mut f.snapshot).unwrap();
                    snap.default_foreground = Rgb(255, 0, 0);
                    snap.default_background = Rgb(0, 0, 0);
                    snap.cursor.blinking = false;
                    for spans in &mut f.rows {
                        for span in Arc::make_mut(spans) {
                            span.style.attrs = attrs::STRIKE | attrs::OVERLINE;
                            span.style.set_underline(kind);
                            span.style.underline_color = TerminalColor::Rgb(Rgb(0, 255, 0));
                            let layout = Arc::make_mut(&mut span.layout);
                            layout.metrics.underline_y_px = 8.0;
                            layout.metrics.underline_thickness_px = 1.0;
                            layout.metrics.strike_y_px = 5.0;
                            layout.metrics.strike_thickness_px = 1.0;
                        }
                    }
                    let mut sprite_cache = crate::sprites::Cache::default();
                    let glyph = sprite_cache.get(0x2588, f.metrics).unwrap();
                    Arc::make_mut(&mut f.rows[0])[0].sprites =
                        Arc::from(vec![crate::sprites::Instance { column: 2, width: 1, glyph }]);
                    let mut h = Harness::new(Fixture { frame: owned });
                    let w = h.window();
                    h.advance(Duration::ZERO);
                    h.focus_window(w, true);
                    h.click(w, Point::new(18., 18.));
                    h.advance(Duration::ZERO);
                    let image = h.render_rgba(w, 1.);
                    let at = |x: u32, y: u32| ((y * image.0 + x) * 4) as usize;
                    for y in [11, 15] {
                        assert_eq!(
                            &image.2[at(20, y)..at(20, y) + 4],
                            &[0, 0, 0, 255],
                            "{kind:?},wide={wide},overhang={overhang},y={y}"
                        );
                    }
                    if kind == hotaru_core::Underline::Single {
                        assert_eq!(&image.2[at(20, 18)..at(20, 18) + 4], &[0, 0, 0, 255]);
                        assert_eq!(&image.2[at(12, 18)..at(12, 18) + 4], &[0, 255, 0, 255]);
                        let expected = if wide { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
                        assert_eq!(
                            &image.2[at(28, 18)..at(28, 18) + 4],
                            &expected,
                            "underline must be below sprite ink"
                        );
                    }
                    assert_eq!(h.check_frame_exact(w, 1.).differing_pixels, 0);
                    images.push(image);
                }
                assert_eq!(images[0], images[1], "decoration paint paths differ for {kind:?},wide={wide}");
            }
        }
    }

    #[test]
    fn block_background_precedes_neighbor_ink_and_tints_only_cursor_source_cell() {
        for cursor_row in [1, 2] {
            let mut owned = frame(true);
            let frame = Arc::get_mut(&mut owned).unwrap();
            let snapshot = Arc::get_mut(&mut frame.snapshot).unwrap();
            snapshot.cursor.row = cursor_row;
            snapshot.cursor.col = 1;
            snapshot.cursor.visible = true;
            snapshot.cursor.blinking = true;
            snapshot.cursor.in_viewport = true;
            snapshot.default_foreground = Rgb(255, 0, 0);
            snapshot.default_background = Rgb(0, 0, 0);
            let mut h = Harness::new(Fixture { frame: owned.clone() });
            let w = h.window();
            h.advance(Duration::ZERO);
            h.focus_window(w, true);
            h.click(w, Point::new(18.0, 18.0));
            h.advance(Duration::ZERO);
            assert_eq!(h.focused_name(w).as_deref(), Some("grid"));
            let (width, _, rgba) = h.render_rgba(w, 1.0);
            let sprite = &owned.rows[1][0].sprites[0];
            let mask = &sprite.glyph.mask;
            let ox = PAD as i32 + 8 + sprite.glyph.origin_x;
            let oy = PAD as i32 + 12 + sprite.glyph.origin_y;
            let mut outside = 0;
            for y in 0..mask.height() {
                for x in 0..mask.width() {
                    let alpha = mask.alpha_bytes()[(y * mask.width() + x) as usize];
                    if alpha != 255 {
                        continue;
                    }
                    let px = ox + x as i32;
                    let py = oy + y as i32;
                    let at = ((py as u32 * width + px as u32) * 4) as usize;
                    let expected = if cursor_row == 1 { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
                    assert_eq!(&rgba[at..at + 4], &expected, "source row1 versus cursor row{cursor_row}, {px},{py}");
                    if py >= PAD as i32 + 24 {
                        outside += 1;
                    }
                }
            }
            assert!(outside > 0, "fixture must cross into blank next-row cursor");
            assert_eq!(h.check_frame_exact(w, 1.0).differing_pixels, 0);
            h.advance(Duration::from_millis(600));
            assert_eq!(h.check_frame_exact(w, 1.0).differing_pixels, 0, "blink must repaint padded extent");
        }
    }

    #[test]
    fn padded_ink_survives_neighbor_backgrounds_and_transition_damage() {
        let normal = frame(false);
        let with_ink = frame(true);
        assert!(scroll_reuse(&normal, &with_ink).is_none());
        assert!(scroll_reuse(&with_ink, &normal).is_none());
        let mut h = Harness::new(Fixture { frame: normal.clone() });
        let w = h.window();
        h.advance(Duration::ZERO);
        let initial = h.render_rgba(w, 1.0);
        assert_eq!(h.check_frame_exact(w, 1.0).differing_pixels, 0);
        h.send(Msg::Shaped(1, with_ink.clone()));
        h.advance(Duration::ZERO);
        assert_eq!(h.check_frame_exact(w, 1.0).differing_pixels, 0);
        let (width, _, rgba) = h.render_rgba(w, 1.0);
        let sprite = &with_ink.rows[1][0].sprites[0];
        let origin_x = PAD as i32 + 8 + sprite.glyph.origin_x;
        let origin_y = PAD as i32 + 12 + sprite.glyph.origin_y;
        let mask = &sprite.glyph.mask;
        for y in 0..mask.height() {
            for x in 0..mask.width() {
                let a = mask.alpha_bytes()[(y * mask.width() + x) as usize];
                let at = (((origin_y + y as i32) as u32 * width + (origin_x + x as i32) as u32) * 4) as usize;
                let blue = ((u32::from(200u8) * (255 - u32::from(a)) + 255 * u32::from(a) + 127) / 255) as u8;
                assert_eq!(&rgba[at..at + 4], &[a, a, blue, 255], "mask {x},{y}");
            }
        }
        h.send(Msg::Shaped(1, normal));
        h.advance(Duration::ZERO);
        assert_eq!(
            h.check_frame_exact(w, 1.0).differing_pixels,
            0,
            "old overhang must be erased even when prior retention was rejected"
        );
        assert_eq!(h.render_rgba(w, 1.0), initial);
    }
}

#[cfg(test)]
mod text_selection_tests {
    use super::*;

    fn lines(terminal: &mut hotaru_core::Terminal, range: std::ops::Range<usize>) {
        for i in range {
            terminal.feed(format!("row {i:02}\r\n").as_bytes());
        }
    }

    #[test]
    fn a_selection_stays_on_its_text_as_output_scrolls_it() {
        let mut terminal = hotaru_core::Terminal::new(20, 10, 100);
        lines(&mut terminal, 0..8);
        let before = terminal.snapshot();
        let shown = Selection { anchor: CellPoint { row: 2, col: 0 }, end: CellPoint { row: 4, col: 6 } };
        let selection = TextSelection::on(shown, &before.position);
        assert_eq!(selection.text(&before), "row 02\nrow 03\nrow 04");
        // Four more rows scroll the text up three screen rows.
        lines(&mut terminal, 8..12);
        let after = terminal.snapshot();
        let moved = selection.visible(&after).unwrap();
        assert_eq!((moved.anchor.row, moved.end.row), (0, 1));
        assert_eq!(moved.text(&after), "row 03\nrow 04", "the part still on the screen");
        assert!(!selection.shown_whole(&after));
        // All of it is in a capture of the content, history included.
        let content = terminal.content_snapshot();
        let (start, end) = selection.in_content(&content).unwrap();
        assert_eq!((start, end), ((2, 0), (4, 6)));
        // Scrolled back, the screen shows all of it again.
        let back = terminal.snapshot_scrolled(1);
        assert!(selection.shown_whole(&back));
        assert_eq!(selection.text(&back), "row 02\nrow 03\nrow 04");
    }

    #[test]
    fn a_selection_off_the_screen_or_on_other_text_shows_nothing() {
        let mut terminal = hotaru_core::Terminal::new(20, 4, 100);
        lines(&mut terminal, 0..2);
        let before = terminal.snapshot();
        let shown = Selection { anchor: CellPoint { row: 0, col: 0 }, end: CellPoint { row: 0, col: 6 } };
        let selection = TextSelection::on(shown, &before.position);
        lines(&mut terminal, 2..10);
        assert!(selection.visible(&terminal.snapshot()).is_none());
        terminal.reset();
        assert!(selection.visible(&terminal.snapshot()).is_none());
        assert!(selection.in_content(&terminal.content_snapshot()).is_none());
    }
}
