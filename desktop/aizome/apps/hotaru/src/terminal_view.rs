//! One native widget per terminal, with cached text and damage by shared row.

use std::sync::Arc;
use std::time::Duration;

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
) -> Element<Msg> {
    let snapshot = rendered.as_ref().map_or(snapshot, |frame| frame.snapshot.clone());
    let (font_size, font_weight) = font;
    Element::new(TerminalView { id, snapshot, font_size, font_weight, selection, bindings, rendered }, vec![])
}

struct TerminalView {
    id: u64,
    snapshot: Arc<Snapshot>,
    font_size: f64,
    font_weight: u16,
    selection: Option<Selection>,
    bindings: Arc<Bindings>,
    rendered: Option<Arc<crate::font::Frame>>,
}

#[derive(Default)]
struct ViewState {
    // Keep the preceding rows alive until layout has compared them. Raw address
    // caches can mistake allocator reuse for unchanged content between frames.
    snapshot: Option<Arc<Snapshot>>,
    cursor: Option<(usize, usize, bool)>,
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
}

fn emit(cx: &mut EventCx, msg: Msg) {
    cx.emit(&MsgFn::new(msg));
}

impl TerminalView {
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
    fn cursor_rect(&self, state: &ViewState) -> Rect {
        let c = &self.snapshot.cursor;
        Rect::from_origin_size(
            (state.padding + c.col as f64 * state.cell_width, state.padding + c.row as f64 * state.line_height),
            (state.cell_width, state.line_height),
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
        let cursor = (self.snapshot.cursor.col, self.snapshot.cursor.row, self.snapshot.cursor.visible);
        let theme = (cx.theme().is_dark(), rgb(cx.theme().text()), rgb(cx.theme().surface()));
        let st = cx.state::<ViewState>();
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
        let old_cursor = st.cursor.replace(cursor);
        st.size = size;
        st.cell_width = cell_width;
        st.line_height = line_height;
        st.resize_pending |= resized;
        st.theme_pending |= st.theme.replace(theme) != Some(theme);
        if st.resize_pending || old_cursor != Some(cursor) || st.theme_pending || st.scale_pending {
            cx.request_tick(Duration::ZERO);
        }
        for (y, row) in self.snapshot.lines.iter().enumerate() {
            if font_changed
                || !old_snapshot
                    .as_ref()
                    .and_then(|s| s.lines.get(y))
                    .is_some_and(|old| hotaru_core::RowRef::ptr_eq(old, row))
            {
                cx.invalidate_rect(Rect::new(
                    0.0,
                    pad + y as f64 * line_height,
                    size.width,
                    pad + (y + 1) as f64 * line_height,
                ));
            }
        }
        if old_cursor != Some(cursor) {
            for (_, y, _) in old_cursor.into_iter().chain([cursor]) {
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
        for (y, spans) in self.rendered.iter().flat_map(|frame| frame.rows.iter().enumerate()) {
            let row_rect = Rect::new(pad, pad + y as f64 * lh, bounds.width() - pad, pad + (y + 1) as f64 * lh);
            if !cx.canvas.needs(row_rect) {
                continue;
            }
            for span in spans {
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
                let r = Rect::new(pad + span.start as f64 * cw, row_rect.y0, pad + span.end as f64 * cw, row_rect.y1);
                cx.canvas.fill_rect(r, bg);
                if style.attrs & attrs::HIDDEN != 0 {
                    continue;
                }
                cx.canvas.save();
                cx.canvas.clip_rect(r);
                cx.canvas.draw_grid_text(&span.layout, Point::new(pad, r.y0), fg);
                if style.attrs & attrs::STRIKE != 0 {
                    let m = span.layout.metrics;
                    let y = r.y0 + f64::from(m.strike_y_px) / m.scale;
                    cx.canvas.fill_rect(Rect::new(r.x0, y, r.x1, y + f64::from(m.strike_thickness_px) / m.scale), fg);
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
        if self.snapshot.cursor.visible {
            let r = self.cursor_rect(state);
            if cx.is_focused() {
                cx.canvas.fill_rect(Rect::new(r.x0, r.y1 - 2.0, r.x1, r.y1), theme.focus());
            } else {
                aizome_kit::draw::border(cx.canvas, r, theme.text3(), 1.0);
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
            Event::Tick => {
                let st = cx.state::<ViewState>();
                let cols = ((st.size.width - st.padding * 2.0) / st.cell_width).floor().max(1.0) as usize;
                let rows = ((st.size.height - st.padding * 2.0) / st.line_height).floor().max(1.0) as usize;
                let resized = std::mem::take(&mut st.resize_pending);
                let scale_changed = std::mem::take(&mut st.scale_pending);
                let scale = st.scale;
                let theme_changed = std::mem::take(&mut st.theme_pending);
                let rect = self.cursor_rect(st);
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
            Event::Ime(Ime::Commit(text)) => emit(cx, Msg::Input(self.id, text.as_bytes().to_vec())),
            Event::Ime(Ime::Preedit { text, .. }) => {
                cx.state::<ViewState>().preedit = text.clone();
                cx.invalidate();
            }
            Event::PointerDown { pos, button, mods, .. } => {
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
                let st = cx.state::<ViewState>();
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
            Event::Scroll { pos, delta, mods, .. } => {
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
                let rect = self.cursor_rect(cx.state::<ViewState>());
                cx.set_text_input(*focused, Some(rect));
                cx.invalidate();
                if *focused && matches!(event, Event::Focus(_)) {
                    emit(cx, Msg::FocusPane(self.id));
                }
                if !focused {
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
