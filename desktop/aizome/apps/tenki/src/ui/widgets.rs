//! Tenki's own widgets: the sidebar's list of places (with dragging to
//! reorder), an hour of the hourly strip, a day of the ten, the gauges of
//! the detail tiles (UV, humidity, the wind's compass, the sun's arc), and
//! two layouts (a grid of tiles, and two columns that stack when narrow).
//!
//! Each draws itself whole from what the view gives it, in whole points,
//! so the kit's damage tracking redraws only the ones that change.

use std::rc::Rc;

use aizome_kit::draw;
use aizome_kit::element::{MsgFn, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Align, Constraints, Point, Rect, Size};
use aizome_kit::menu::MenuSpec;
use aizome_kit::sidestep_ui::kurbo::{BezPath, Circle, Line, Shape};
use aizome_kit::sidestep_ui::{
    Cursor, Key, LineCap, LineJoin, LinearGradient, NamedKey, Paint, PointerButton, StrokeStyle,
};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Role, Theme};
use aizome_kit::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Color, Element, Icon, Layoutable, Length, Menu};

fn mono(size: f64, weight: f32) -> FontSpec {
    FontSpec { weight, tabular: true, ..FontSpec::mono(size) }
}

fn ui(size: f64, weight: f32) -> FontSpec {
    FontSpec { weight, ..FontSpec::ui(size) }
}

/// The role a weather glyph is drawn in: amber for the sun, hanada for
/// what falls, the quiet text colour for the rest.
pub fn glyph_role(icon: Icon) -> Role {
    match icon {
        Icon::Sun | Icon::CloudSun | Icon::CloudSunRain | Icon::Sunrise | Icon::Sunset => Role::Warning,
        Icon::CloudRain
        | Icon::CloudDrizzle
        | Icon::CloudHail
        | Icon::CloudSnow
        | Icon::Snowflake
        | Icon::CloudLightning
        | Icon::CloudMoonRain
        | Icon::Umbrella
        | Icon::Droplets
        | Icon::Droplet => Role::Accent,
        _ => Role::Text2,
    }
}

pub fn glyph_color(t: &Theme, icon: Icon) -> Color {
    t.role(glyph_role(icon))
}

fn emit<T: Copy + 'static>(cx: &mut EventCx, map: &MsgMap<T>, v: T) {
    let m = map.clone();
    cx.emit(&MsgFn::from_fn(move || m.make(v)));
}

// The list of places.

/// A row's height in the list of places.
pub const PLACE_ROW: f64 = 48.0;
const DRAG_SLOP: f64 = 4.0;

/// What the list says about each row when it floats under the pointer.
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct RowLabel {
    pub name: String,
    pub detail: String,
}

/// The places, `rows` (one element each, keyed), the one showing
/// `selected`: a click shows a row, a drag moves it (the first `pinned`
/// stay put), arrows choose, ⌥ and arrows move, Delete removes, and a
/// right click opens `menu`.
#[allow(clippy::too_many_arguments)]
pub fn place_list<Msg: Clone + Send + 'static>(
    rows: Vec<Element<Msg>>,
    labels: Vec<RowLabel>,
    selected: Option<usize>,
    pinned: usize,
    on_select: impl Fn(usize) -> Msg + 'static,
    on_move: impl Fn((usize, usize)) -> Msg + 'static,
    on_remove: impl Fn(usize) -> Msg + 'static,
    menu: impl Fn(usize) -> Menu<Msg> + 'static,
) -> Element<Msg> {
    let w = PlaceList {
        n: rows.len(),
        labels,
        selected,
        pinned,
        on_select: MsgMap::new(on_select),
        on_move: MsgMap::new(on_move),
        on_remove: MsgMap::new(on_remove),
        menu: Rc::new(move |i| menu(i).spec().clone()),
        slots: Vec::new(),
    };
    Element::new(w, rows).key("places").width(Length::Fill)
}

struct PlaceList {
    n: usize,
    labels: Vec<RowLabel>,
    selected: Option<usize>,
    pinned: usize,
    on_select: MsgMap<usize>,
    on_move: MsgMap<(usize, usize)>,
    on_remove: MsgMap<usize>,
    menu: Rc<dyn Fn(usize) -> MenuSpec>,
    /// Where each row sits, as laid out (rows shift while one is dragged).
    slots: Vec<usize>,
}

#[derive(Default, Debug)]
struct ListState {
    /// The row pressed, and where.
    press: Option<(usize, Point)>,
    drag: Option<Dragging>,
    hover: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Dragging {
    from: usize,
    /// The pointer's height in the row it took.
    grab: f64,
    /// The pointer's height in the list.
    y: f64,
}

impl Dragging {
    /// Where the row would land.
    fn to(&self, n: usize, pinned: usize) -> usize {
        let top = self.y - self.grab;
        ((top / PLACE_ROW).round().max(0.0) as usize).clamp(pinned, n.saturating_sub(1).max(pinned))
    }

    /// The floating row's top, kept inside the movable rows.
    fn top(&self, n: usize, pinned: usize) -> f64 {
        (self.y - self.grab).clamp(pinned as f64 * PLACE_ROW, (n.saturating_sub(1)) as f64 * PLACE_ROW)
    }
}

/// Each row's slot with row `from` taken to `to`.
fn slots(n: usize, drag: Option<(usize, usize)>) -> Vec<usize> {
    (0..n)
        .map(|i| match drag {
            Some((from, to)) if i == from => to,
            Some((from, to)) if from < to && i > from && i <= to => i - 1,
            Some((from, to)) if to < from && i >= to && i < from => i + 1,
            _ => i,
        })
        .collect()
}

impl PlaceList {
    fn row_at(&self, y: f64) -> Option<usize> {
        let slot = (y / PLACE_ROW).floor();
        if slot < 0.0 {
            return None;
        }
        let slot = slot as usize;
        self.slots.iter().position(|&s| s == slot).or((slot < self.n).then_some(slot))
    }
}

impl Widget for PlaceList {
    fn name(&self) -> &'static str {
        "tenki-places"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let width = if bc.max.width.is_finite() { bc.max.width } else { 240.0 };
        let drag = cx.state::<ListState>().drag.map(|d| (d.from, d.to(self.n, self.pinned)));
        self.slots = slots(self.n, drag);
        for i in 0..cx.child_count().min(self.n) {
            let sel = self.selected == Some(i);
            cx.layout_child_selected(i, Constraints::tight(Size::new(width, PLACE_ROW)), sel);
            cx.place_child(i, Point::new(0.0, self.slots[i] as f64 * PLACE_ROW));
        }
        bc.constrain(Size::new(width, self.n as f64 * PLACE_ROW))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let w = cx.size().width;
        let st = cx.state::<ListState>();
        let dragging = st.is_some_and(|s| s.drag.is_some());
        if let Some(h) = st.and_then(|s| s.hover).filter(|h| Some(*h) != self.selected && !dragging) {
            let y = self.slots.get(h).copied().unwrap_or(h) as f64 * PLACE_ROW;
            cx.canvas.fill_rect(Rect::new(0.0, y, w, y + PLACE_ROW), t.hover(t.sunk()));
        }
        if let Some(s) = self.selected.filter(|s| *s < self.n) {
            let y = self.slots.get(s).copied().unwrap_or(s) as f64 * PLACE_ROW;
            let r = Rect::new(0.0, y, w, y + PLACE_ROW);
            cx.canvas.fill_rect(r, t.selection());
            if cx.focus_visible() {
                draw::border(cx.canvas, r.inset(-1.0), t.focus(), 2.0);
            }
        }
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let Some(d) = cx.state::<ListState>().and_then(|s| s.drag) else { return };
        let t = cx.theme();
        let w = cx.size().width;
        // The gap the row will land in, quietened.
        let to = d.to(self.n, self.pinned);
        let gap = Rect::new(0.0, to as f64 * PLACE_ROW, w, (to + 1) as f64 * PLACE_ROW);
        cx.canvas.fill_rect(gap, t.sunk().with_alpha(0.82));
        draw::border(cx.canvas, gap.inset(-1.0), t.line2(), 1.0);
        // The row itself, floating under the pointer.
        let top = d.top(self.n, self.pinned).round();
        let r = Rect::new(4.0, top, w - 4.0, top + PLACE_ROW);
        draw::floating(cx.canvas, t, r);
        draw::left(cx.canvas, r, t.focus(), 3.0);
        if let Some(l) = self.labels.get(d.from) {
            let name = TextSpec::new(ui(t.metrics.body, 600.0), t.text());
            let detail = TextSpec::new(ui(t.metrics.small, 400.0), t.text3());
            cx.draw_text_in(&l.name, &name, Rect::new(r.x0 + 12.0, r.y0 + 7.0, r.x1 - 10.0, r.y0 + 25.0), Align::Start);
            cx.draw_text_in(
                &l.detail,
                &detail,
                Rect::new(r.x0 + 12.0, r.y0 + 25.0, r.x1 - 10.0, r.y0 + 41.0),
                Align::Start,
            );
        }
    }

    fn overflow(&self) -> f64 {
        3.0
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.n, self.selected, self.pinned, &self.slots, &self.labels));
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn cursor(&self) -> Option<Cursor> {
        None
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                let Some(i) = self.row_at(pos.y).filter(|i| *i < self.n) else { return Status::Ignored };
                cx.request_focus();
                emit(cx, &self.on_select, i);
                let st = cx.state::<ListState>();
                st.press = Some((i, *pos));
                st.drag = None;
                cx.capture_pointer();
                Status::Handled
            }
            Event::PointerDown { pos, button: PointerButton::Right, .. } => {
                let Some(i) = self.row_at(pos.y).filter(|i| *i < self.n) else { return Status::Ignored };
                emit(cx, &self.on_select, i);
                let menu = (self.menu)(i);
                cx.open_context_menu(menu, *pos);
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let (n, pinned) = (self.n, self.pinned);
                let hover = self.row_at(pos.y).filter(|i| *i < n);
                let st = cx.state::<ListState>();
                let mut changed = st.hover != hover;
                st.hover = hover;
                if let Some((i, start)) = st.press {
                    if st.drag.is_none() && (pos.y - start.y).abs() > DRAG_SLOP && i >= pinned && n > pinned + 1 {
                        st.drag = Some(Dragging { from: i, grab: start.y - i as f64 * PLACE_ROW, y: pos.y });
                    }
                    if let Some(d) = &mut st.drag {
                        d.y = pos.y;
                        changed = true;
                        cx.set_cursor(Cursor::Grabbing);
                        cx.relayout();
                    }
                }
                if changed {
                    cx.invalidate();
                }
                Status::Handled
            }
            Event::PointerUp { button: PointerButton::Left, .. } => {
                let (n, pinned) = (self.n, self.pinned);
                let st = cx.state::<ListState>();
                st.press = None;
                let drag = st.drag.take();
                cx.release_pointer();
                if let Some(d) = drag {
                    let to = d.to(n, pinned);
                    if to != d.from {
                        emit(cx, &self.on_move, (d.from, to));
                    }
                    cx.set_cursor(Cursor::Default);
                    cx.relayout();
                    cx.invalidate();
                }
                Status::Handled
            }
            Event::PointerLeave => {
                let st = cx.state::<ListState>();
                if st.hover.take().is_some() {
                    cx.invalidate();
                }
                Status::Ignored
            }
            Event::Key(k) if k.pressed && cx.is_focused() => {
                let i = self.selected.unwrap_or(0);
                let alt = k.modifiers.alt();
                if k.modifiers.super_key() || k.modifiers.control() {
                    return Status::Ignored;
                }
                match &k.key {
                    Key::Named(NamedKey::Escape) => {
                        let st = cx.state::<ListState>();
                        if st.drag.take().is_some() {
                            st.press = None;
                            cx.release_pointer();
                            cx.relayout();
                            cx.invalidate();
                            return Status::Handled;
                        }
                        Status::Ignored
                    }
                    Key::Named(NamedKey::ArrowUp) if alt => {
                        if i > self.pinned && i < self.n {
                            emit(cx, &self.on_move, (i, i - 1));
                        }
                        Status::Handled
                    }
                    Key::Named(NamedKey::ArrowDown) if alt => {
                        if i >= self.pinned && i + 1 < self.n {
                            emit(cx, &self.on_move, (i, i + 1));
                        }
                        Status::Handled
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        if i > 0 {
                            emit(cx, &self.on_select, i - 1);
                        }
                        Status::Handled
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if i + 1 < self.n {
                            emit(cx, &self.on_select, i + 1);
                        }
                        Status::Handled
                    }
                    Key::Named(NamedKey::Home) => {
                        emit(cx, &self.on_select, 0);
                        Status::Handled
                    }
                    Key::Named(NamedKey::End) if self.n > 0 => {
                        emit(cx, &self.on_select, self.n - 1);
                        Status::Handled
                    }
                    Key::Named(NamedKey::Delete | NamedKey::Backspace) if self.selected.is_some() => {
                        emit(cx, &self.on_remove, i);
                        Status::Handled
                    }
                    _ => Status::Ignored,
                }
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::List, label: Some("Places".into()), ..Default::default() })
    }
}

// A clickable row (search results).

/// `child` as a row that answers a click with `msg`, lit under the
/// pointer and by the keyboard's focus.
pub fn click_row<Msg: Clone + Send + 'static>(child: impl Into<Element<Msg>>, msg: Msg, label: String) -> Element<Msg> {
    Element::new(ClickRow { msg: MsgFn::new(msg), label }, vec![child.into()]).width(Length::Fill)
}

struct ClickRow {
    msg: MsgFn,
    label: String,
}

impl Widget for ClickRow {
    fn name(&self) -> &'static str {
        "tenki-click-row"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let width = if bc.max.width.is_finite() { bc.max.width } else { 240.0 };
        let s = cx.layout_child(0, Constraints { min: Size::new(width, 0.0), max: Size::new(width, bc.max.height) });
        cx.place_child(0, Point::ZERO);
        bc.constrain(Size::new(width, s.height.max(cx.theme().metrics.target)))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        if cx.is_pressed() {
            cx.canvas.fill_rect(r, t.pressed(t.sunk()));
        } else if cx.is_hovered() {
            cx.canvas.fill_rect(r, t.hover(t.sunk()));
        }
        if cx.focus_visible() {
            draw::border(cx.canvas, r.inset(-1.0), t.focus(), 2.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.label);
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Tab
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerUp { button: PointerButton::Left, pos, .. } if cx.bounds().contains(*pos) => {
                cx.emit(&self.msg);
                Status::Handled
            }
            Event::PointerDown { button: PointerButton::Left, .. } => Status::Handled,
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        cx.emit(&self.msg);
        true
    }

    fn cursor(&self) -> Option<Cursor> {
        Some(Cursor::Pointer)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Button,
            label: Some(self.label.clone()),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }
}

// An hour of the hourly strip.

pub const HOUR_WIDTH: f64 = 56.0;
pub const HOUR_HEIGHT: f64 = 124.0;
/// An hour's height when none of the hours shown has rain: no bars.
pub const HOUR_HEIGHT_DRY: f64 = 78.0;

/// One hour: its time (or "Now", or the day it starts), its glyph, its
/// temperature, and a bar of how likely rain (or snow) is.
#[derive(Clone, Debug, PartialEq)]
pub struct HourCell {
    pub label: String,
    /// "Now", or the first hour of a day: set brighter.
    pub strong: bool,
    /// The first hour of a new day: a rule at its left.
    pub day_start: bool,
    pub icon: Icon,
    pub temp: String,
    /// Percent.
    pub probability: Option<f64>,
    /// Room for the chance of rain (some hour shown has some).
    pub bars: bool,
    pub label_for_access: String,
}

impl HourCell {
    fn height(&self) -> f64 {
        if self.bars { HOUR_HEIGHT } else { HOUR_HEIGHT_DRY }
    }
}

pub fn hour_cell<Msg>(cell: HourCell) -> Element<Msg> {
    let h = cell.height();
    Element::new(cell, Vec::new()).width(HOUR_WIDTH).height(h)
}

impl Widget for HourCell {
    fn name(&self) -> &'static str {
        "tenki-hour"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(HOUR_WIDTH, self.height()))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let w = cx.size().width;
        if self.day_start {
            cx.canvas.fill_rect(Rect::new(0.0, 0.0, 1.0, self.height()), t.line2());
        }
        let label = TextSpec::new(
            mono(11.0, if self.strong { 600.0 } else { 400.0 }),
            if self.strong { t.text() } else { t.text3() },
        );
        cx.draw_text_in(&self.label, &label, Rect::new(0.0, 0.0, w, 16.0), Align::Center);
        let s = 22.0;
        self.icon.draw(cx.canvas, Point::new(((w - s) / 2.0).round(), 24.0), s, glyph_color(t, self.icon));
        let temp = TextSpec::new(mono(14.0, 500.0), t.text());
        cx.draw_text_in(&self.temp, &temp, Rect::new(0.0, 54.0, w, 72.0), Align::Center);
        if !self.bars {
            return;
        }
        // The chance of rain: a bar up from a baseline, and its percent.
        let base = 104.0;
        let track = Rect::new((w / 2.0 - 7.0).round(), base - 24.0, (w / 2.0 + 7.0).round(), base);
        cx.canvas.fill_rect(Rect::new(track.x0 - 3.0, base, track.x1 + 3.0, base + 1.0), t.line());
        let blue = aizome_kit::theme::color(t.palette.blue);
        if let Some(p) = self.probability.filter(|p| *p >= 1.0) {
            let h = (track.height() * (p / 100.0).clamp(0.0, 1.0)).round().max(1.0);
            cx.canvas.fill_rect(
                Rect::new(track.x0, base - h, track.x1, base),
                blue.with_alpha(0.35 + 0.6 * (p as f32 / 100.0)),
            );
        }
        let pct = match self.probability {
            Some(p) if p >= 5.0 => format!("{:.0}%", p),
            _ => String::new(),
        };
        if !pct.is_empty() {
            let spec = TextSpec::new(mono(10.0, 500.0), blue);
            cx.draw_text_in(&pct, &spec, Rect::new(0.0, base + 3.0, w, base + 18.0), Align::Center);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(&self.label, self.strong, self.day_start, self.icon, &self.temp, self.bars))
            .f64(self.probability.unwrap_or(-1.0));
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Cell, label: Some(self.label_for_access.clone()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.label)
    }
}

// A day of the ten.

pub const DAY_HEIGHT: f64 = 38.0;

/// One day: its name, glyph, chance of rain, low and high, and the range
/// between them on the week's scale in the dye ramp.
#[derive(Clone, Debug, PartialEq)]
pub struct DayRow {
    pub label: String,
    pub today: bool,
    pub icon: Icon,
    pub probability: Option<f64>,
    /// °C.
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    pub lo_text: String,
    pub hi_text: String,
    /// The coldest and warmest of all the days shown, °C.
    pub span: (f64, f64),
    /// The temperature now, on today's row.
    pub now: Option<f64>,
    /// A rule above it (all but the first).
    pub rule: bool,
    pub label_for_access: String,
}

pub fn day_row<Msg>(row: DayRow) -> Element<Msg> {
    Element::new(row, Vec::new()).width(Length::Fill).height(DAY_HEIGHT)
}

impl DayRow {
    /// Where the range bar runs, in a row `w` wide.
    fn bar(w: f64) -> (f64, f64) {
        (196.0, (w - 58.0).max(240.0))
    }
}

impl Widget for DayRow {
    fn name(&self) -> &'static str {
        "tenki-day"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 480.0 };
        bc.constrain(Size::new(w, DAY_HEIGHT))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let w = cx.size().width;
        let h = DAY_HEIGHT;
        if self.rule {
            cx.canvas.fill_rect(Rect::new(0.0, 0.0, w, 1.0), t.line());
        }
        let row = |x0: f64, x1: f64| Rect::new(x0, 1.0, x1, h);
        let name = TextSpec::new(ui(t.metrics.body, if self.today { 600.0 } else { 400.0 }), t.text());
        cx.draw_text_in(&self.label, &name, row(0.0, 64.0), Align::Start);
        let s = 20.0;
        self.icon.draw(cx.canvas, Point::new(72.0, ((h - s) / 2.0).round() + 1.0), s, glyph_color(t, self.icon));
        let blue = aizome_kit::theme::color(t.palette.blue);
        if let Some(p) = self.probability.filter(|p| *p >= 10.0) {
            let spec = TextSpec::new(mono(11.0, 500.0), blue);
            cx.draw_text_in(&format!("{p:.0}%"), &spec, row(98.0, 138.0), Align::Start);
        }
        let (x0, x1) = DayRow::bar(w);
        let lo = TextSpec::new(mono(13.0, 400.0), t.text3());
        let hi = TextSpec::new(mono(13.0, 600.0), t.text());
        cx.draw_text_in(&self.lo_text, &lo, row(x0 - 50.0, x0 - 10.0), Align::End);
        cx.draw_text_in(&self.hi_text, &hi, row(x1 + 10.0, x1 + 50.0), Align::End);
        // The track, the week's scale.
        let cy = (h / 2.0).round() + 1.0;
        let track = Rect::new(x0, cy - 3.0, x1, cy + 3.0);
        cx.canvas.fill_rect(track, t.sunk());
        draw::border(cx.canvas, track.inflate(1.0, 1.0), t.line(), 1.0);
        let (smin, smax) = self.span;
        let range = (smax - smin).max(1.0);
        let x_of = |c: f64| (x0 + (x1 - x0) * ((c - smin) / range).clamp(0.0, 1.0)).round();
        if let (Some(lo), Some(hi)) = (self.lo, self.hi) {
            let (a, b) = (x_of(lo), x_of(hi).max(x_of(lo) + 4.0));
            let g = LinearGradient {
                start: Point::new(x0, cy),
                end: Point::new(x1, cy),
                stops: crate::weather::ramp_gradient(t, smin, smin + range),
            };
            cx.canvas.fill(&Rect::new(a, track.y0, b, track.y1), Paint::Linear(g));
        }
        if let Some(n) = self.now {
            let x = x_of(n);
            let outer = Rect::new(x - 5.0, cy - 5.0, x + 5.0, cy + 5.0);
            cx.canvas.fill_rect(outer, t.chrome());
            cx.canvas.fill_rect(outer.inset(-1.0), t.text());
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(&self.label, self.today, self.icon, &self.lo_text, &self.hi_text, self.rule));
        for v in [self.probability, self.lo, self.hi, Some(self.span.0), Some(self.span.1), self.now] {
            h.f64(v.unwrap_or(f64::NAN));
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Row, label: Some(self.label_for_access.clone()), ..Default::default() })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.label)
    }
}

// Gauges for the tiles.

/// A bar along a scale: the UV index on its bands, or how humid it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gauge {
    /// A marker at the index on a track of the WHO's bands, 0 to 11+.
    Uv(f64),
    /// The track filled in hanada to a share (0–1).
    Fill(f64),
}

pub fn gauge<Msg>(g: Gauge) -> Element<Msg> {
    Element::new(g, Vec::new()).width(Length::Fill).height(12.0)
}

impl Widget for Gauge {
    fn name(&self) -> &'static str {
        "tenki-gauge"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 160.0 };
        bc.constrain(Size::new(w, 12.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let w = cx.size().width;
        let track = Rect::new(1.0, 3.0, w - 1.0, 9.0);
        match *self {
            Gauge::Uv(uv) => {
                let at = uv / 11.0;
                let g = LinearGradient {
                    start: Point::new(track.x0, 6.0),
                    end: Point::new(track.x1, 6.0),
                    stops: crate::weather::uv_stops(t),
                };
                cx.canvas.fill(&track, Paint::Linear(g));
                let x = (track.x0 + track.width() * at.clamp(0.0, 1.0)).round();
                let m = Rect::new(x - 4.0, 0.0, x + 4.0, 12.0);
                cx.canvas.fill_rect(m, t.chrome());
                cx.canvas.fill_rect(m.inset(-2.0), t.text());
            }
            Gauge::Fill(at) => {
                let color = t.role(Role::Accent);
                cx.canvas.fill_rect(track, t.sunk());
                draw::border(cx.canvas, track.inflate(1.0, 1.0), t.line(), 1.0);
                let x = (track.x0 + track.width() * at.clamp(0.0, 1.0)).round();
                cx.canvas.fill_rect(Rect::new(track.x0, track.y0, x, track.y1), color);
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        match *self {
            Gauge::Uv(v) => h.add(&0u8).f64(v),
            Gauge::Fill(v) => h.add(&1u8).f64(v),
        };
    }
}

/// The wind's compass: north up, an arrow the way the wind blows.
#[derive(Clone, Debug, PartialEq)]
pub struct Compass {
    /// Where the wind comes from, degrees clockwise from north.
    pub from: Option<f64>,
}

pub const COMPASS: f64 = 64.0;

pub fn compass<Msg>(from: Option<f64>) -> Element<Msg> {
    Element::new(Compass { from }, Vec::new()).width(COMPASS).height(COMPASS)
}

impl Widget for Compass {
    fn name(&self) -> &'static str {
        "tenki-compass"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(COMPASS, COMPASS))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let c = Point::new(COMPASS / 2.0, COMPASS / 2.0);
        let r = COMPASS / 2.0 - 8.0;
        cx.canvas.stroke(&Circle::new(c, r), &StrokeStyle::new(1.0), t.line2());
        let spec = TextSpec::new(mono(9.0, 600.0), t.text3());
        for (label, dx, dy) in [("N", 0.0, -1.0), ("E", 1.0, 0.0), ("S", 0.0, 1.0), ("W", -1.0, 0.0)] {
            let p = Point::new(c.x + dx * (r + 1.0), c.y + dy * (r + 1.0));
            let box_ = Rect::new(p.x - 6.0, p.y - 6.0, p.x + 6.0, p.y + 6.0);
            cx.canvas.fill_rect(box_, t.chrome());
            cx.draw_text_in(label, &spec, box_, Align::Center);
        }
        let Some(from) = self.from else { return };
        // The wind blows toward the opposite of where it comes from.
        let a = (from + 180.0).to_radians();
        let v = (a.sin(), -a.cos());
        let len = r - 6.0;
        let head = Point::new(c.x + v.0 * len, c.y + v.1 * len);
        let tail = Point::new(c.x - v.0 * len, c.y - v.1 * len);
        let color = t.text();
        let style = StrokeStyle::new(2.0).with_cap(LineCap::Square).with_join(LineJoin::Miter);
        cx.canvas.stroke(&Line::new(tail, head), &style, color);
        // The head: a triangle back from the tip, 9pt long and 9pt across.
        let base = Point::new(head.x - v.0 * 9.0, head.y - v.1 * 9.0);
        let side = |s: f64| Point::new(base.x - v.1 * 4.5 * s, base.y + v.0 * 4.5 * s);
        let mut arrow = BezPath::new();
        arrow.move_to(head);
        arrow.line_to(side(1.0));
        arrow.line_to(side(-1.0));
        arrow.close_path();
        cx.canvas.fill(&arrow, color);
        let tail_box = Rect::new(tail.x - 2.5, tail.y - 2.5, tail.x + 2.5, tail.y + 2.5);
        cx.canvas.fill_rect(tail_box, color);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.from.unwrap_or(f64::NAN));
    }

    fn semantics(&self) -> Option<Semantics> {
        let label = self.from.map(|f| format!("Wind from {}", crate::weather::compass(f)));
        Some(Semantics { role: SemanticRole::Image, label, ..Default::default() })
    }
}

/// The sun's arc from sunrise to sunset, and where it is now.
#[derive(Clone, Debug, PartialEq)]
pub struct SunArc {
    /// How far through the day's light it is: 0 at sunrise, 1 at sunset;
    /// outside that, night.
    pub at: Option<f64>,
}

pub fn sun_arc<Msg>(at: Option<f64>) -> Element<Msg> {
    Element::new(SunArc { at }, Vec::new()).width(Length::Fill).height(40.0)
}

impl Widget for SunArc {
    fn name(&self) -> &'static str {
        "tenki-sun-arc"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 160.0 };
        bc.constrain(Size::new(w, 40.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let w = cx.size().width;
        let horizon = 34.0;
        let (x0, x1) = (8.0, w - 8.0);
        cx.canvas.fill_rect(Rect::new(0.0, horizon, w, horizon + 1.0), t.line2());
        let point = |f: f64| {
            let x = x0 + (x1 - x0) * f;
            let y = horizon - (std::f64::consts::PI * f).sin() * (horizon - 6.0);
            Point::new(x, y)
        };
        let mut arc = BezPath::new();
        arc.move_to(point(0.0));
        for i in 1..=48 {
            arc.line_to(point(f64::from(i) / 48.0));
        }
        let dash = StrokeStyle::new(1.0).with_dash(vec![3.0, 3.0], 0.0);
        cx.canvas.stroke(&arc, &dash, t.faint());
        let amber = aizome_kit::theme::color(t.palette.amber);
        match self.at {
            Some(f) if (0.0..=1.0).contains(&f) => {
                // The part of the day gone, solid.
                let mut done = BezPath::new();
                done.move_to(point(0.0));
                let steps = (48.0 * f).ceil().max(1.0) as usize;
                for i in 1..=steps {
                    done.line_to(point(f * i as f64 / steps as f64));
                }
                cx.canvas.stroke(&done, &StrokeStyle::new(1.5), amber);
                let p = point(f);
                cx.canvas.fill(&Circle::new(p, 5.0).to_path(0.1), amber);
                cx.canvas.stroke(&Circle::new(p, 6.0), &StrokeStyle::new(1.5), t.chrome());
            }
            _ => {
                for f in [0.0, 1.0] {
                    let p = point(f);
                    cx.canvas.fill_rect(Rect::new(p.x - 3.0, p.y - 3.0, p.x + 3.0, p.y + 3.0), t.faint());
                }
            }
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.at.unwrap_or(f64::NAN));
    }
}

// Layouts.

/// Children in as many equal columns as fit at `min` points each, rows as
/// tall as their tallest.
pub fn grid<Msg>(children: Vec<Element<Msg>>, min: f64, gap: f64) -> Element<Msg> {
    Element::new(Grid { min, gap }, children).width(Length::Fill)
}

struct Grid {
    min: f64,
    gap: f64,
}

impl Widget for Grid {
    fn name(&self) -> &'static str {
        "tenki-grid"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { self.min * 3.0 + self.gap * 2.0 };
        let n = cx.child_count();
        let cols = (((w + self.gap) / (self.min + self.gap)).floor() as usize).clamp(1, n.max(1));
        let col_w = ((w - self.gap * (cols as f64 - 1.0)) / cols as f64).floor();
        let mut y = 0.0;
        let mut i = 0;
        while i < n {
            let row: Vec<usize> = (i..(i + cols).min(n)).collect();
            let mut h: f64 = 0.0;
            for &j in &row {
                h = h.max(
                    cx.layout_child(
                        j,
                        Constraints { min: Size::new(col_w, 0.0), max: Size::new(col_w, f64::INFINITY) },
                    )
                    .height,
                );
            }
            for (k, &j) in row.iter().enumerate() {
                // Every tile in a row as tall as the tallest.
                cx.layout_child(j, Constraints::tight(Size::new(col_w, h)));
                let x = if k + 1 == cols { w - col_w } else { k as f64 * (col_w + self.gap) };
                cx.place_child(j, Point::new(x.round(), y));
            }
            y += h + self.gap;
            i += cols;
        }
        bc.constrain(Size::new(w, (y - self.gap).max(0.0)))
    }
}

/// Two children side by side when there's `breakpoint` points for them
/// (the first `share` of the width), else one above the other.
pub fn columns<Msg>(first: Element<Msg>, second: Element<Msg>, breakpoint: f64, share: f64, gap: f64) -> Element<Msg> {
    Element::new(Columns { breakpoint, share, gap }, vec![first, second]).width(Length::Fill)
}

struct Columns {
    breakpoint: f64,
    share: f64,
    gap: f64,
}

impl Widget for Columns {
    fn name(&self) -> &'static str {
        "tenki-columns"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { self.breakpoint };
        let loose = |width: f64| Constraints { min: Size::new(width, 0.0), max: Size::new(width, f64::INFINITY) };
        if w >= self.breakpoint {
            let a = ((w - self.gap) * self.share).floor();
            let b = w - self.gap - a;
            let sa = cx.layout_child(0, loose(a));
            let sb = cx.layout_child(1, loose(b));
            cx.place_child(0, Point::ZERO);
            cx.place_child(1, Point::new(a + self.gap, 0.0));
            bc.constrain(Size::new(w, sa.height.max(sb.height)))
        } else {
            let sa = cx.layout_child(0, loose(w));
            let sb = cx.layout_child(1, loose(w));
            cx.place_child(0, Point::ZERO);
            cx.place_child(1, Point::new(0.0, sa.height + self.gap));
            bc.constrain(Size::new(w, sa.height + self.gap + sb.height))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_make_room_for_the_one_dragged() {
        assert_eq!(slots(4, None), vec![0, 1, 2, 3]);
        assert_eq!(slots(4, Some((1, 3))), vec![0, 3, 1, 2]);
        assert_eq!(slots(4, Some((3, 1))), vec![0, 2, 3, 1]);
        let d = Dragging { from: 1, grab: 10.0, y: 3.0 * PLACE_ROW + 30.0 };
        assert_eq!(d.to(4, 1), 3);
        let d = Dragging { from: 2, grab: 10.0, y: -100.0 };
        assert_eq!(d.to(4, 1), 1, "the pinned row stays first");
        assert_eq!(d.top(4, 1), PLACE_ROW);
    }
}
