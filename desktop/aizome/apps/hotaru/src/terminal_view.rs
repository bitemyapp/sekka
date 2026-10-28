//! One native widget per terminal, with cached text and damage by shared row.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Cursor as PointerCursor, Ime, Key, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    EditCommand, Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, StateView, Status,
    Widget,
};
use hotaru_core::input as vt;
use hotaru_core::{Color as TerminalColor, Rgb, Snapshot, TerminalColors, attrs};

use crate::app::Msg;
use crate::input;
use crate::settings::Bindings;

const PAD: f64 = 10.0;
const CURSOR_TIMER: u32 = 0;
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

pub fn terminal_view(
    id: u64,
    snapshot: Arc<Snapshot>,
    font: (f64, u16),
    selection: Option<Selection>,
    bindings: Arc<Bindings>,
    rendered: Option<Arc<crate::font::Frame>>,
    input_activity: u64,
) -> Element<Msg> {
    let snapshot = rendered.as_ref().map_or(snapshot, |frame| frame.snapshot.clone());
    let (font_size, font_weight) = font;
    Element::new(
        TerminalView { id, snapshot, font_size, font_weight, selection, bindings, rendered, input_activity },
        vec![],
    )
}

struct TerminalView {
    id: u64,
    snapshot: Arc<Snapshot>,
    font_size: f64,
    font_weight: u16,
    selection: Option<Selection>,
    bindings: Arc<Bindings>,
    rendered: Option<Arc<crate::font::Frame>>,
    input_activity: u64,
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
    drag: Option<CellPoint>,
    preedit: String,
    scroll_fraction: f64,
    resize_pending: bool,
    theme: Option<(bool, Rgb, Rgb)>,
    theme_pending: bool,
    window_rect: Rect,
    keys: Vec<(u32, vt::KeyEvent)>,
    buttons: Vec<vt::MouseButton>,
    hovered: Option<crate::links::Link>,
    link_press: Option<crate::links::Press>,
    pointer: Option<Point>,
    link_modifier: bool,
    hover_clear_pending: bool,
    sprite_overhang: bool,
}

fn retained_frame(frame: Option<&Arc<crate::font::Frame>>) -> Option<Arc<crate::font::Frame>> {
    frame.filter(|frame| frame.retention_bytes <= SCROLL_FRAME_BYTES).cloned()
}

// Position metadata proposes a translation. Owned row and shaped-run identity
// prove each retained row; anything unproven is rasterized after the copy.
fn scroll_reuse(old: &crate::font::Frame, new: &crate::font::Frame) -> Option<(isize, Vec<bool>)> {
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
    if delta == 0 || delta.unsigned_abs() >= b.rows {
        return None;
    }
    if old.rows.iter().chain(&new.rows).flat_map(|r| r.iter()).any(|s| s.selected || s.matched || s.current_match) {
        return None;
    }
    let clean: Vec<_> = (0..b.rows)
        .map(|y| {
            let Some(source) = y.checked_add_signed(-delta).filter(|source| *source < a.rows) else {
                return false;
            };
            hotaru_core::RowRef::ptr_eq(&a.lines[source], &b.lines[y]) && Arc::ptr_eq(&old.rows[source], &new.rows[y])
        })
        .collect();
    (clean.iter().filter(|clean| **clean).count() >= 2).then_some((delta, clean))
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
        let hit =
            pos.and_then(|pos| self.link_point(pos, st)).and_then(|cell| crate::links::Link::at(&self.snapshot, cell));
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
        let old_snapshot = st.snapshot.replace(self.snapshot.clone());
        if old_snapshot.as_ref().is_some_and(|old| !Arc::ptr_eq(old, &self.snapshot)) {
            st.hover_clear_pending |= st.hovered.take().is_some();
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
        if let Some((delta, _)) = &reuse {
            let dy = *delta as f64 * line_height;
            let mut fixed = Vec::with_capacity(3);
            // Repaint the old cursor AND where its old pixels were copied.
            // Full rows include wide tails, hollow borders and changed glyph
            // shaping boundaries. This is safe for every previous blink phase.
            for y in old_cursor.into_iter().map(|c| c.row).chain([cursor.row]) {
                if y < self.snapshot.rows {
                    fixed.push(Rect::new(
                        pad,
                        pad + y as f64 * line_height,
                        content.x1,
                        pad + (y + 1) as f64 * line_height,
                    ));
                }
            }
            if let Some(y) =
                old_cursor.and_then(|c| c.row.checked_add_signed(*delta)).filter(|y| *y < self.snapshot.rows)
            {
                fixed.push(Rect::new(
                    pad,
                    pad + y as f64 * line_height,
                    content.x1,
                    pad + (y + 1) as f64 * line_height,
                ));
            }
            cx.scrolled(content, Vec2::new(0.0, dy), &fixed);
        }
        for (y, row) in self.snapshot.lines.iter().enumerate() {
            let unchanged = reuse.as_ref().map_or_else(
                || {
                    old_snapshot
                        .as_ref()
                        .and_then(|s| s.lines.get(y))
                        .is_some_and(|old| hotaru_core::RowRef::ptr_eq(old, row))
                },
                |(_, clean)| clean[y],
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
                    if style.attrs & attrs::STRIKE != 0 {
                        let m = span.layout.metrics;
                        let y = r.y0 + f64::from(m.strike_y_px) / m.scale;
                        cx.canvas
                            .fill_rect(Rect::new(r.x0, y, r.x1, y + f64::from(m.strike_thickness_px) / m.scale), fg);
                    }
                    if style.attrs & attrs::OVERLINE != 0 {
                        cx.canvas.fill_rect(Rect::new(r.x0, r.y0 + 1.0, r.x1, r.y0 + 2.0), fg);
                    }
                    let underline_color = if style.underline_color == TerminalColor::Default {
                        fg
                    } else {
                        color(style.underline_color, true)
                    };
                    underline(cx.canvas, r, style.underline(), underline_color, span.layout.metrics);
                    cx.canvas.restore();
                }
            }
        }
        if let Some(hovered) = &state.hovered
            && state.link_modifier
            && hovered.matches_snapshot(&self.snapshot)
            && let Some(frame) = &self.rendered
        {
            for span in &frame.links {
                if self.snapshot.lines[span.row]
                    .hyperlink(span.start)
                    .is_some_and(|link| link.id == hovered.identity && link.uri.as_ref() == hovered.target.as_bytes())
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
                if key.pressed && key.modifiers.shift() {
                    use aizome_kit::sidestep_ui::NamedKey;
                    let lines = match key.key {
                        Key::Named(NamedKey::PageUp) => Some(self.snapshot.rows as i32),
                        Key::Named(NamedKey::PageDown) => Some(-(self.snapshot.rows as i32)),
                        _ => None,
                    };
                    if let Some(lines) = lines {
                        emit(cx, Msg::Scroll(self.id, lines));
                        return Status::Handled;
                    }
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
            Event::PointerDown { pos, button, mods, .. } => {
                self.hover(cx, Some(*pos), mods.super_key());
                if *button == PointerButton::Left
                    && mods.super_key()
                    && let Some(link) = cx.state::<ViewState>().hovered.clone()
                {
                    let st = cx.state::<ViewState>();
                    st.drag = None;
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
                let select = *button == PointerButton::Left
                    && (mods.shift() || self.snapshot.modes.mouse_tracking == vt::MouseTracking::None);
                if select {
                    st.drag = Some(p);
                }
                cx.request_focus();
                cx.capture_pointer();
                emit(cx, Msg::FocusPane(self.id));
                if !mods.shift() {
                    emit(cx, Msg::Mouse(self.id, pointer));
                }
                if select {
                    emit(cx, Msg::Select(self.id, Selection { anchor: p, end: p }));
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
                let selection = st.drag.map(|anchor| Selection { anchor, end: self.point(*pos, st) });
                if !mods.shift() {
                    emit(cx, Msg::Mouse(self.id, pointer));
                }
                if let Some(selection) = selection {
                    emit(cx, Msg::Select(self.id, selection));
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
                st.drag = None;
                let pointer = self.mouse_input(st, *pos, *mods, vt::MouseAction::Release, button);
                if !mods.shift() {
                    emit(cx, Msg::Mouse(self.id, pointer));
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
            EditCommand::Copy => self.selection.is_some_and(|s| !s.is_empty()),
            EditCommand::Paste | EditCommand::Find => true,
            _ => false,
        }
    }
    fn command(&mut self, cx: &mut EventCx, command: EditCommand) -> bool {
        match command {
            EditCommand::Copy => emit(cx, Msg::Copy),
            EditCommand::Paste => emit(cx, Msg::Paste),
            EditCommand::Find => emit(cx, Msg::Find),
            _ => return false,
        }
        true
    }
    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::MultilineTextInput, label: Some("Terminal".into()), ..Default::default() })
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
        h.send(Msg::CloseTab);
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
