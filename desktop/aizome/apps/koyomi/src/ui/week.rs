//! The week and day views: a header of days, a strip of all-day events,
//! and the time grid.
//!
//! The time grid is one widget drawing the hours and columns, with each
//! event a child block it places (a block that didn't change isn't
//! drawn again) and the now-line a child of its own. It handles the
//! pointer itself: pressing a block selects it, dragging moves it,
//! dragging its bottom edge resizes it, dragging across empty time makes
//! an event. While a drag is under way nothing is viewed or laid out
//! again: the grid draws the ghost from its own state and invalidates
//! only where the ghost was and is.

use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::draw;
use aizome_kit::element::{Element, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Length, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Color, Cursor, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use aizome_kit::{Align, Layoutable};
use jiff::civil::{Date, DateTime, Time};

use crate::model::Dye;
use crate::ui::colors;

/// The hour labels' column.
pub const GUTTER: f64 = 56.0;
/// One hour of the grid.
pub const HOUR: f64 = 48.0;
/// Times snap to this many minutes.
pub const SNAP: f64 = 15.0;
/// The smallest a block is drawn (fifteen minutes and a bit).
const MIN_BLOCK: f64 = 18.0;
/// How near a block's bottom edge the pointer resizes it.
const EDGE: f64 = 6.0;
/// How far the pointer moves before a press becomes a drag.
const SLOP: f64 = 3.0;
/// An all-day lane.
pub const LANE: f64 = 20.0;

/// What the grid tells the program.
#[derive(Clone, Debug, PartialEq)]
pub enum GridAction {
    /// An event pressed (or none: empty time pressed).
    Select(Option<u64>),
    /// An event double-clicked.
    Open(u64),
    /// A day's empty space pressed.
    Day(Date),
    /// Time dragged across, or double-clicked: a new event.
    Create { start: DateTime, end: DateTime, all_day: bool },
    /// An event dropped somewhere else.
    Move { key: u64, start: DateTime, end: DateTime, all_day: bool },
    /// An event's end dragged.
    Resize { key: u64, end: DateTime },
    /// Show this day in the day view ("+3 more", a year's day).
    ShowDay(Date),
    /// Show this day's week.
    ShowWeek(Date),
    /// Show this day's month.
    ShowMonth(Date),
}

/// A timed event in the grid.
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Block {
    pub key: u64,
    /// Its column.
    pub day: usize,
    /// Minutes from the column's midnight.
    pub start: i32,
    pub end: i32,
    /// Its place among the events it overlaps.
    pub col: u16,
    pub cols: u16,
    pub title: Arc<str>,
    pub time: Arc<str>,
    pub location: Arc<str>,
    pub dye: Dye,
    pub selected: bool,
    pub cancelled: bool,
    /// Its real start and end (it may run past the column).
    pub real_start: DateTime,
    pub real_end: DateTime,
}

/// Everything the time grid shows.
#[derive(Clone, Debug, PartialEq)]
pub struct GridData {
    pub days: Vec<Date>,
    pub today: Date,
    pub now: DateTime,
    pub h24: bool,
    pub blocks: Vec<Block>,
    /// Which columns are weekends.
    pub weekend: Vec<bool>,
    /// A scroll the program asks for: a token (acted on once) and the
    /// minute to show at the top.
    pub scroll_to: (u64, i32),
}

/// Where things are in the grid.
#[derive(Clone, Copy, Debug)]
struct Geo {
    width: f64,
    height: f64,
    n: usize,
    scroll: f64,
}

impl Geo {
    fn col_x(&self, i: usize) -> (f64, f64) {
        let w = (self.width - GUTTER).max(1.0) / self.n.max(1) as f64;
        ((GUTTER + w * i as f64).round(), (GUTTER + w * (i + 1) as f64).round())
    }

    fn col_at(&self, x: f64) -> Option<usize> {
        if x < GUTTER {
            return None;
        }
        let w = (self.width - GUTTER).max(1.0) / self.n.max(1) as f64;
        Some((((x - GUTTER) / w) as usize).min(self.n.saturating_sub(1)))
    }

    fn y(&self, minute: f64) -> f64 {
        (minute / 60.0 * HOUR - self.scroll).round()
    }

    fn minute_at(&self, y: f64) -> f64 {
        (y + self.scroll) / HOUR * 60.0
    }

    fn max_scroll(&self) -> f64 {
        (24.0 * HOUR + 1.0 - self.height).max(0.0)
    }

    fn block_rect(&self, b: &Block) -> Rect {
        let (x0, x1) = self.col_x(b.day);
        let inner = (x1 - x0 - 4.0).max(4.0);
        let w = inner / b.cols.max(1) as f64;
        let bx0 = (x0 + 2.0 + w * b.col as f64).round();
        let bx1 = (x0 + 2.0 + w * (b.col + 1) as f64).round() - if b.col + 1 < b.cols { 1.0 } else { 0.0 };
        let y0 = self.y(b.start as f64) + 1.0;
        let y1 = (self.y(b.end as f64)).max(y0 + MIN_BLOCK);
        Rect::new(bx0, y0, bx1, y1)
    }

    fn ghost_rect(&self, day: usize, start: f64, end: f64) -> Rect {
        let (x0, x1) = self.col_x(day);
        let y0 = self.y(start) + 1.0;
        let y1 = self.y(end).max(y0 + MIN_BLOCK);
        Rect::new(x0 + 2.0, y0, x1 - 2.0, y1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragKind {
    Move,
    Resize,
    Create,
}

#[derive(Clone, Debug)]
struct Drag {
    kind: DragKind,
    /// The block (none when making one).
    block: Option<usize>,
    origin: Point,
    /// Minutes between the press and the block's start (moving).
    grab: f64,
    active: bool,
    /// Where the ghost is: column, start and end minutes.
    ghost: Option<(usize, f64, f64)>,
}

#[derive(Default)]
struct GridState {
    scroll: f64,
    /// The scroll request last acted on.
    token: Option<u64>,
    drag: Option<Drag>,
    /// The pointer is over a block's bottom edge.
    edge: bool,
}

struct Grid<Msg> {
    data: Rc<GridData>,
    on: MsgMap<GridAction>,
    geo: Geo,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

/// The time grid.
pub fn grid<Msg: Send + 'static>(data: Rc<GridData>, on: impl Fn(GridAction) -> Msg + 'static) -> Element<Msg> {
    let mut children: Vec<Element<Msg>> = data
        .blocks
        .iter()
        .map(|b| Element::new(BlockView { b: b.clone() }, Vec::new()).key(("block", b.key, b.day)))
        .collect();
    let label = crate::format::time(data.now.time(), data.h24);
    let today = data.days.iter().position(|d| *d == data.today);
    children.push(Element::new(NowLine { label, today, n: data.days.len().max(1) }, Vec::new()).key("now-line"));
    Element::new(
        Grid::<Msg> {
            data,
            on: MsgMap::new(on),
            geo: Geo { width: 0.0, height: 0.0, n: 1, scroll: 0.0 },
            _m: std::marker::PhantomData,
        },
        children,
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .id("time-grid")
}

fn snap(m: f64) -> f64 {
    (m / SNAP).round() * SNAP
}

fn at(day: Date, minute: f64) -> DateTime {
    let m = minute.clamp(0.0, 24.0 * 60.0) as i64;
    let base = day.to_datetime(Time::midnight());
    base.checked_add(jiff::SignedDuration::from_mins(m)).unwrap_or(base)
}

fn mono(size: f64, color: Color) -> TextSpec {
    TextSpec::new(FontSpec { tabular: true, ..FontSpec::mono(size) }, color)
}

fn ui(size: f64, weight: f32, color: Color) -> TextSpec {
    let mut f = FontSpec::ui(size);
    f.weight = weight;
    TextSpec::new(f, color)
}

impl<Msg: Send + 'static> Grid<Msg> {
    fn hit_block(&self, p: Point) -> Option<(usize, bool)> {
        // Later blocks are drawn over earlier ones.
        for (i, b) in self.data.blocks.iter().enumerate().rev() {
            let r = self.geo.block_rect(b);
            if r.contains(p) {
                return Some((i, p.y >= r.y1 - EDGE));
            }
        }
        None
    }

    fn ghost_bounds(&self, g: (usize, f64, f64)) -> Rect {
        // The ghost and its time label above it.
        self.geo.ghost_rect(g.0, g.1, g.2).inflate(4.0, 4.0)
    }

    fn set_ghost(&self, cx: &mut EventCx, ghost: Option<(usize, f64, f64)>) {
        let old = cx.state::<GridState>().drag.as_ref().and_then(|d| d.ghost);
        if old == ghost {
            return;
        }
        if let Some(o) = old {
            cx.invalidate_rect(self.ghost_bounds(o));
        }
        if let Some(n) = ghost {
            cx.invalidate_rect(self.ghost_bounds(n));
        }
        if let Some(d) = cx.state::<GridState>().drag.as_mut() {
            d.ghost = ghost;
        }
    }

    fn scroll_by(&self, cx: &mut EventCx, dy: f64) -> bool {
        let max = self.geo.max_scroll();
        let st = cx.state::<GridState>();
        let s = (st.scroll + dy).clamp(0.0, max).round();
        if s == st.scroll {
            return false;
        }
        st.scroll = s;
        cx.relayout();
        cx.invalidate();
        true
    }

    fn emit(&self, cx: &mut EventCx, a: GridAction) {
        super::send(cx, self.on.make(a));
    }
}

impl<Msg: Send + 'static> Widget for Grid<Msg> {
    fn name(&self) -> &'static str {
        "koyomi-time-grid"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 800.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 600.0 },
        );
        let mut geo = Geo { width: size.width, height: size.height, n: self.data.days.len().max(1), scroll: 0.0 };
        let (token, minute) = self.data.scroll_to;
        let st = cx.state::<GridState>();
        if st.token != Some(token) {
            st.token = Some(token);
            st.scroll = (minute as f64 / 60.0 * HOUR).round();
        }
        st.scroll = st.scroll.clamp(0.0, geo.max_scroll());
        geo.scroll = st.scroll;
        self.geo = geo;
        for (i, b) in self.data.blocks.iter().enumerate() {
            let r = geo.block_rect(b);
            cx.layout_child(i, Constraints::tight(r.size()));
            cx.place_child(i, r.origin());
        }
        // The now-line, across the week, if today is in it.
        let n = self.data.blocks.len();
        let now_min = (self.data.now.hour() as f64) * 60.0 + self.data.now.minute() as f64;
        let y = geo.y(now_min);
        if self.data.days.contains(&self.data.today) && y > -8.0 && y < size.height + 8.0 {
            cx.layout_child(n, Constraints::tight(Size::new(size.width, 13.0)));
            cx.place_child(n, Point::new(0.0, y - 6.0));
        } else {
            cx.layout_child(n, Constraints::tight(Size::ZERO));
            cx.place_child(n, Point::new(0.0, -100.0));
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let geo = self.geo;
        let b = cx.bounds();
        let weekend = t.surface().mix(t.sunk(), if t.is_dark() { 0.45 } else { 0.55 });
        for (i, d) in self.data.days.iter().enumerate() {
            let (x0, x1) = geo.col_x(i);
            let r = Rect::new(x0, 0.0, x1, b.y1);
            if !cx.canvas.needs(r) {
                continue;
            }
            if *d == self.data.today {
                cx.canvas.fill_rect(r, t.surface().mix(t.focus(), if t.is_dark() { 0.025 } else { 0.07 }));
            } else if self.data.weekend.get(i).copied().unwrap_or(false) {
                cx.canvas.fill_rect(r, weekend);
            }
        }
        // Half hours, hours, columns.
        let half = t.line().with_alpha(0.45);
        let first = (geo.scroll / HOUR).floor() as i32;
        let last = ((geo.scroll + b.height()) / HOUR).ceil() as i32;
        for h in first.max(0)..=last.min(24) {
            let y = geo.y(h as f64 * 60.0);
            let hy = geo.y(h as f64 * 60.0 + 30.0);
            if h < 24 && cx.canvas.needs(Rect::new(GUTTER, hy, b.x1, hy + 1.0)) {
                cx.canvas.fill_rect(Rect::new(GUTTER, hy, b.x1, hy + 1.0), half);
            }
            if cx.canvas.needs(Rect::new(GUTTER - 6.0, y, b.x1, y + 1.0)) {
                cx.canvas.fill_rect(Rect::new(GUTTER - 6.0, y, b.x1, y + 1.0), t.line());
            }
            // The hour's label, beside its line (not at midnight's top).
            if h > 0 && h < 24 {
                let r = Rect::new(4.0, y - 8.0, GUTTER - 10.0, y + 8.0);
                if cx.canvas.needs(r) {
                    let label = crate::format::hour(h as i8, self.data.h24);
                    cx.draw_text_in(&label, &mono(10.5, t.text3()), r, Align::End);
                }
            }
        }
        for i in 0..=geo.n {
            let x = if i == geo.n { b.x1 - 1.0 } else { geo.col_x(i).0 };
            if i == geo.n && geo.n > 1 {
                continue;
            }
            cx.canvas.fill_rect(Rect::new(x, 0.0, x + 1.0, b.y1), t.line());
        }
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let geo = self.geo;
        // A thin bar showing where in the day the grid is.
        let total = 24.0 * HOUR;
        let b = cx.bounds();
        if total > b.height() {
            let h = (b.height() / total * b.height()).max(24.0);
            let y = geo.scroll / (total - b.height()).max(1.0) * (b.height() - h);
            let r = Rect::new(b.x1 - 4.0, y.round(), b.x1 - 1.0, (y + h).round());
            if cx.canvas.needs(r) {
                cx.canvas.fill_rect(r, t.line2().with_alpha(0.7));
            }
        }
        let Some(st) = cx.state::<GridState>() else { return };
        let Some(d) = &st.drag else { return };
        let Some((day, s, e)) = d.ghost else { return };
        let block = d.block.and_then(|i| self.data.blocks.get(i));
        // The block being moved fades where it was.
        if let (Some(bl), DragKind::Move) = (block, d.kind) {
            let r = geo.block_rect(bl);
            cx.canvas.fill_rect(r, t.surface().with_alpha(0.55));
        }
        let r = geo.ghost_rect(day, s, e);
        let fill = match block {
            Some(bl) => colors::dye(t, bl.dye),
            None => t.focus(),
        };
        let on = colors::on_dye(t, fill);
        cx.canvas.fill_rect(r, fill.with_alpha(if block.is_some() { 0.92 } else { 0.35 }));
        draw::border(cx.canvas, r, t.focus(), 1.0);
        if t.shadows() {
            let s3 = t.metrics.shadow;
            cx.canvas
                .fill_rect(Rect::new(r.x0 + s3, r.y1, r.x1 + s3, r.y1 + s3).intersect(r.inflate(3.0, 3.0)), t.slab());
            cx.canvas.fill_rect(Rect::new(r.x1, r.y0 + s3, r.x1 + s3, r.y1).intersect(r.inflate(3.0, 3.0)), t.slab());
        }
        let label = crate::format::time_range(
            at(self.data.days[day], s).time(),
            at(self.data.days[day], e).time(),
            self.data.h24,
        );
        let fg = if block.is_some() { on } else { t.text() };
        cx.draw_text_in(
            &label,
            &mono(10.5, fg),
            Rect::new(r.x0 + 6.0, r.y0 + 2.0, r.x1 - 4.0, r.y0 + 16.0),
            Align::Start,
        );
        if let Some(bl) = block {
            cx.draw_text_in(
                &bl.title,
                &ui(12.0, 600.0, fg),
                Rect::new(r.x0 + 6.0, r.y0 + 15.0, r.x1 - 4.0, r.y0 + 31.0).intersect(r),
                Align::Start,
            );
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        // The grid's own drawing: not the blocks (children), not the
        // drag (its own state, invalidated as it moves).
        h.add(&self.data.days).add(&self.data.today).add(&self.data.weekend).add(&self.data.h24);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn clips(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::No
    }

    fn cursor(&self) -> Option<Cursor> {
        None
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Scroll { delta, .. } => {
                self.scroll_by(cx, delta.y);
                Status::Handled
            }
            Event::PointerDown { pos, button: PointerButton::Left, clicks, .. } => {
                let p = *pos;
                let Some(col) = self.geo.col_at(p.x) else { return Status::Handled };
                let day = self.data.days[col];
                let minute = self.geo.minute_at(p.y);
                match self.hit_block(p) {
                    Some((i, _)) if *clicks >= 2 => {
                        self.emit(cx, GridAction::Open(self.data.blocks[i].key));
                    }
                    Some((i, edge)) => {
                        let b = &self.data.blocks[i];
                        let start_here = if b.day == col { b.start as f64 } else { minute };
                        cx.state::<GridState>().drag = Some(Drag {
                            kind: if edge { DragKind::Resize } else { DragKind::Move },
                            block: Some(i),
                            origin: p,
                            grab: minute - start_here,
                            active: false,
                            ghost: None,
                        });
                        cx.capture_pointer();
                        if !b.selected {
                            self.emit(cx, GridAction::Select(Some(b.key)));
                        }
                    }
                    None if *clicks >= 2 => {
                        let s = (minute / 30.0).floor() * 30.0;
                        self.emit(
                            cx,
                            GridAction::Create {
                                start: at(day, s),
                                end: at(day, (s + 60.0).min(1440.0)),
                                all_day: false,
                            },
                        );
                    }
                    None => {
                        cx.state::<GridState>().drag = Some(Drag {
                            kind: DragKind::Create,
                            block: None,
                            origin: p,
                            grab: 0.0,
                            active: false,
                            ghost: None,
                        });
                        cx.capture_pointer();
                        self.emit(cx, GridAction::Day(day));
                    }
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let p = *pos;
                let Some(d) = cx.state::<GridState>().drag.clone() else {
                    let edge = matches!(self.hit_block(p), Some((_, true)));
                    if edge != cx.state::<GridState>().edge {
                        cx.state::<GridState>().edge = edge;
                        cx.set_cursor(if edge { Cursor::RowResize } else { Cursor::Default });
                    }
                    return Status::Ignored;
                };
                if !d.active && (p - d.origin).hypot() < SLOP {
                    return Status::Handled;
                }
                if !d.active
                    && let Some(dd) = cx.state::<GridState>().drag.as_mut()
                {
                    dd.active = true;
                }
                // Scroll when held near the top or bottom.
                if p.y < 16.0 {
                    self.scroll_by(cx, -(16.0 - p.y).min(16.0));
                } else if p.y > self.geo.height - 16.0 {
                    self.scroll_by(cx, (p.y - self.geo.height + 16.0).min(16.0));
                }
                let scroll = cx.state::<GridState>().scroll;
                let geo = Geo { scroll, ..self.geo };
                let minute = geo.minute_at(p.y.clamp(0.0, geo.height));
                let col = geo.col_at(p.x.max(GUTTER)).unwrap_or(0);
                let ghost = match (d.kind, d.block.and_then(|i| self.data.blocks.get(i))) {
                    (DragKind::Move, Some(b)) => {
                        let len = (b.real_end.duration_since(b.real_start).as_secs() as f64 / 60.0).max(SNAP);
                        let s = snap(minute - d.grab).clamp(0.0, 1440.0 - SNAP);
                        Some((col, s, (s + len).min(1440.0)))
                    }
                    (DragKind::Resize, Some(b)) => {
                        let e = snap(minute).clamp(b.start as f64 + SNAP, 1440.0);
                        Some((b.day, b.start as f64, e))
                    }
                    (DragKind::Create, _) => {
                        let first = geo.minute_at(d.origin.y);
                        let (a, z) = if minute >= first { (first, minute) } else { (minute, first) };
                        let col0 = geo.col_at(d.origin.x).unwrap_or(0);
                        let (s, e) = (
                            (a / SNAP).floor() * SNAP,
                            ((z / SNAP).ceil() * SNAP).max((a / SNAP).floor() * SNAP + SNAP),
                        );
                        Some((col0, s, e.min(1440.0)))
                    }
                    _ => None,
                };
                // The geometry the ghost is drawn with follows the scroll.
                self.geo = geo;
                self.set_ghost(cx, ghost);
                Status::Handled
            }
            Event::PointerUp { button: PointerButton::Left, .. } => {
                let Some(d) = cx.state::<GridState>().drag.clone() else { return Status::Ignored };
                self.set_ghost(cx, None);
                if let (Some(bl), DragKind::Move) = (d.block.and_then(|i| self.data.blocks.get(i)), d.kind) {
                    cx.invalidate_rect(self.geo.block_rect(bl));
                }
                cx.state::<GridState>().drag = None;
                cx.release_pointer();
                if !d.active {
                    return Status::Handled;
                }
                let Some((col, s, e)) = d.ghost else { return Status::Handled };
                let day = self.data.days[col];
                match (d.kind, d.block.and_then(|i| self.data.blocks.get(i))) {
                    (DragKind::Move, Some(b)) => {
                        let start = at(day, s);
                        let len = b.real_end.duration_since(b.real_start);
                        let end = start.checked_add(len).unwrap_or(start);
                        if start != b.real_start {
                            self.emit(cx, GridAction::Move { key: b.key, start, end, all_day: false });
                        }
                    }
                    (DragKind::Resize, Some(b)) => {
                        let end = at(self.data.days[b.day], e);
                        if end != b.real_end {
                            self.emit(cx, GridAction::Resize { key: b.key, end });
                        }
                    }
                    (DragKind::Create, _) => {
                        self.emit(cx, GridAction::Create { start: at(day, s), end: at(day, e), all_day: false });
                    }
                    _ => {}
                }
                Status::Handled
            }
            Event::PointerLeave => {
                if std::mem::take(&mut cx.state::<GridState>().edge) {
                    cx.set_cursor(Cursor::Default);
                }
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Table, label: Some("Time grid".into()), ..Default::default() })
    }
}

/// An event's block.
struct BlockView {
    b: Block,
}

impl Widget for BlockView {
    fn name(&self) -> &'static str {
        "koyomi-block"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.min
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let dye = colors::dye(t, self.b.dye);
        let (fill, fg, sub) = if self.b.selected {
            let on = colors::on_dye(t, dye);
            (dye, on, on.with_alpha(0.8))
        } else {
            (colors::tint(t, dye), colors::on_tint(t, dye), t.text2())
        };
        cx.canvas.fill_rect(r, fill);
        // The calendar's colour down the left edge.
        cx.canvas.fill_rect(
            Rect::new(r.x0, r.y0, r.x0 + 3.0, r.y1),
            if self.b.selected { fg.with_alpha(0.55) } else { dye },
        );
        if self.b.selected {
            draw::border(cx.canvas, r, t.bevel_lo().with_alpha(0.6), 1.0);
        } else {
            // A hairline in the dye keeps blocks crisp where they meet.
            let edge = dye.with_alpha(if t.is_dark() { 0.35 } else { 0.4 });
            draw::top(cx.canvas, r, edge, 1.0);
            draw::bottom(cx.canvas, r, edge, 1.0);
            draw::right(cx.canvas, r, edge, 1.0);
        }
        let x = r.x0 + 7.0;
        let w = (r.x1 - 3.0).max(x);
        let short = r.height() < 34.0;
        let mut title = ui(12.0, 600.0, fg);
        title.strikethrough = self.b.cancelled;
        if short {
            // One line: the title, then the time if there's room.
            let tw = cx.draw_text_in(&self.b.title, &title, Rect::new(x, r.y0 + 1.0, w, r.y0 + 16.0), Align::Start);
            let start = self.b.time.split('–').next().unwrap_or(&self.b.time).to_owned();
            cx.draw_text_in(
                &start,
                &mono(10.5, sub),
                Rect::new(x + tw + 6.0, r.y0 + 1.0, w, r.y0 + 16.0),
                Align::Start,
            );
            return;
        }
        cx.draw_text_in(&self.b.title, &title, Rect::new(x, r.y0 + 3.0, w, r.y0 + 19.0), Align::Start);
        cx.draw_text_in(&self.b.time, &mono(10.5, sub), Rect::new(x, r.y0 + 19.0, w, r.y0 + 33.0), Align::Start);
        if !self.b.location.is_empty() && r.height() >= 50.0 {
            cx.draw_text_in(
                &self.b.location,
                &ui(11.0, 400.0, sub),
                Rect::new(x, r.y0 + 33.0, w, r.y0 + 48.0),
                Align::Start,
            );
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.b.title).add(&self.b.time).add(&self.b.location).add(&(
            self.b.dye,
            self.b.selected,
            self.b.cancelled,
        ));
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Button,
            label: Some(format!("{}, {}", self.b.title, self.b.time)),
            selected: Some(self.b.selected),
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.b.title)
    }
}

/// The amber line at the present minute, with the time in the gutter.
struct NowLine {
    label: String,
    /// Today's column, where the line is strongest.
    today: Option<usize>,
    n: usize,
}

impl Widget for NowLine {
    fn name(&self) -> &'static str {
        "koyomi-now"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.min
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        if r.width() <= 0.0 {
            return;
        }
        let amber = colors::now_line(t);
        let y = 6.0;
        // Faint across the week, strong across today.
        cx.canvas.fill_rect(Rect::new(GUTTER - 4.0, y, r.x1, y + 1.0), amber.with_alpha(0.55));
        if let Some(c) = self.today {
            let geo = Geo { width: r.width(), height: 0.0, n: self.n, scroll: 0.0 };
            let (x0, x1) = geo.col_x(c);
            cx.canvas.fill_rect(Rect::new(x0, y, x1, y + 2.0), amber);
            cx.canvas.fill_rect(Rect::new(x0 - 3.0, y - 3.0, x0 + 3.0, y + 3.0), amber);
        }
        // The time, on an amber chip over the hour labels.
        let chip = Rect::new(2.0, 0.0, GUTTER - 4.0, 14.0);
        cx.canvas.fill_rect(chip, amber);
        let on = colors::on_dye(t, amber);
        cx.draw_text_in(&self.label, &mono(10.0, on), chip, Align::Center);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.str(&self.label).add(&(self.today, self.n));
    }

    fn semantics(&self) -> Option<Semantics> {
        None
    }
}

/// Lay out a day's timed events side by side where they overlap: each
/// gets a column among those its cluster needs.
pub fn pack(blocks: &mut [Block]) {
    blocks.sort_by(|a, b| a.day.cmp(&b.day).then(a.start.cmp(&b.start)).then(b.end.cmp(&a.end)));
    let mut i = 0;
    while i < blocks.len() {
        // A cluster: events that overlap one another, transitively.
        let mut j = i + 1;
        let mut end = blocks[i].end.max(blocks[i].start + 15);
        while j < blocks.len() && blocks[j].day == blocks[i].day && blocks[j].start < end {
            end = end.max(blocks[j].end.max(blocks[j].start + 15));
            j += 1;
        }
        let mut col_ends: Vec<i32> = Vec::new();
        for b in blocks[i..j].iter_mut() {
            let e = b.end.max(b.start + 15);
            match col_ends.iter().position(|&c| c <= b.start) {
                Some(c) => {
                    b.col = c as u16;
                    col_ends[c] = e;
                }
                None => {
                    b.col = col_ends.len() as u16;
                    col_ends.push(e);
                }
            }
        }
        let n = col_ends.len().max(1) as u16;
        for b in blocks[i..j].iter_mut() {
            b.cols = n;
        }
        i = j;
    }
}

/// The days across the top of the week (or day) view.
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct HeaderData {
    pub days: Vec<Date>,
    pub today: Date,
    pub cursor: Date,
    pub week_number: Option<u32>,
}

struct Header<Msg> {
    data: HeaderData,
    on_day: MsgMap<Date>,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

pub const HEADER: f64 = 46.0;

/// The header: each day's name and number (today's on the focus colour).
pub fn header<Msg: Send + 'static>(data: HeaderData, on_day: impl Fn(Date) -> Msg + 'static) -> Element<Msg> {
    Element::new(Header::<Msg> { data, on_day: MsgMap::new(on_day), _m: std::marker::PhantomData }, Vec::new())
        .width(Length::Fill)
        .height(HEADER)
}

impl<Msg: Send + 'static> Widget for Header<Msg> {
    fn name(&self) -> &'static str {
        "koyomi-week-header"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width, HEADER))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let geo = Geo { width: b.width(), height: b.height(), n: self.data.days.len().max(1), scroll: 0.0 };
        if let Some(n) = self.data.week_number {
            cx.draw_text_in(
                &format!("W{n}"),
                &mono(10.5, t.faint()),
                Rect::new(4.0, 6.0, GUTTER - 10.0, 22.0),
                Align::End,
            );
        }
        let single = self.data.days.len() == 1;
        for (i, d) in self.data.days.iter().enumerate() {
            let (x0, x1) = geo.col_x(i);
            let today = *d == self.data.today;
            let name = crate::format::weekday_short(d.weekday()).to_uppercase();
            let mut cap = mono(10.5, if today { t.text() } else { t.text3() });
            cap.letter_spacing = 1.0;
            cx.draw_text_in(&name, &cap, Rect::new(x0 + 8.0, 4.0, x1 - 4.0, 18.0), Align::Start);
            let num = d.day().to_string();
            let spec = TextSpec::new(
                FontSpec { weight: 600.0, tabular: true, ..FontSpec::mono(17.0) },
                if today { t.on_focus() } else { t.text() },
            );
            let nw = cx.layout_text(&num, &spec, None).size().width.ceil();
            let r = Rect::new(x0 + 6.0, 19.0, x0 + 6.0 + nw + 8.0, 41.0);
            if today {
                cx.canvas.fill_rect(r, t.focus());
            } else if *d == self.data.cursor && !single {
                draw::border(cx.canvas, r, t.line2(), 1.0);
            }
            cx.draw_text_in(&num, &spec, r, Align::Center);
            if single {
                let label = format!("{} {}", crate::format::month_name(d.month()), d.year());
                cx.draw_text_in(
                    &label,
                    &ui(13.0, 400.0, t.text2()),
                    Rect::new(r.x1 + 10.0, 21.0, x1 - 8.0, 40.0),
                    Align::Start,
                );
            }
            cx.canvas.fill_rect(Rect::new(x0, 0.0, x0 + 1.0, b.y1), t.line());
        }
        draw::bottom(cx.canvas, b, t.line(), 1.0);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.data);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if let Event::PointerDown { pos, button: PointerButton::Left, .. } = event {
            let geo =
                Geo { width: cx.size().width, height: cx.size().height, n: self.data.days.len().max(1), scroll: 0.0 };
            if let Some(i) = geo.col_at(pos.x) {
                super::send(cx, self.on_day.make(self.data.days[i]));
            }
            return Status::Handled;
        }
        Status::Ignored
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Row, label: Some("Days".into()), ..Default::default() })
    }
}

/// An all-day event in the strip.
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Bar {
    pub key: u64,
    /// The columns it covers (clipped to the week) and its lane.
    pub first: usize,
    pub last: usize,
    pub lane: usize,
    /// It starts before or ends after the week.
    pub before: bool,
    pub after: bool,
    pub title: Arc<str>,
    pub dye: Dye,
    pub selected: bool,
    pub cancelled: bool,
    /// The days it really runs, for moving it.
    pub start: Date,
    pub days: i64,
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct StripData {
    pub days: Vec<Date>,
    pub bars: Vec<Bar>,
    /// Lanes shown (more are counted per day).
    pub lanes: usize,
    pub more: Vec<usize>,
    pub today: Date,
}

impl StripData {
    pub fn height(&self) -> f64 {
        (self.lanes.max(1) as f64 * LANE + 8.0).round()
    }
}

#[derive(Default)]
struct StripState {
    press: Option<(Point, Option<usize>)>,
    ghost: Option<(usize, usize)>,
}

struct Strip<Msg> {
    data: Rc<StripData>,
    on: MsgMap<GridAction>,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

/// The all-day strip above the grid.
pub fn strip<Msg: Send + 'static>(data: Rc<StripData>, on: impl Fn(GridAction) -> Msg + 'static) -> Element<Msg> {
    let h = data.height();
    Element::new(Strip::<Msg> { data, on: MsgMap::new(on), _m: std::marker::PhantomData }, Vec::new())
        .width(Length::Fill)
        .height(h)
        .id("all-day")
}

impl<Msg: Send + 'static> Strip<Msg> {
    fn geo(&self, size: Size) -> Geo {
        Geo { width: size.width, height: size.height, n: self.data.days.len().max(1), scroll: 0.0 }
    }

    fn bar_rect(&self, size: Size, b: &Bar) -> Rect {
        let geo = self.geo(size);
        let x0 = geo.col_x(b.first).0 + if b.before { 0.0 } else { 2.0 };
        let x1 = geo.col_x(b.last).1 - if b.after { 0.0 } else { 2.0 };
        let y = 4.0 + b.lane as f64 * LANE;
        Rect::new(x0, y, x1, y + LANE - 2.0)
    }

    fn hit(&self, size: Size, p: Point) -> Option<usize> {
        self.data.bars.iter().position(|b| b.lane < self.data.lanes && self.bar_rect(size, b).contains(p))
    }

    fn ghost_rect(&self, size: Size, g: (usize, usize)) -> Rect {
        let geo = self.geo(size);
        Rect::new(geo.col_x(g.0).0 + 1.0, 2.0, geo.col_x(g.1).1 - 1.0, size.height - 2.0)
    }

    fn emit(&self, cx: &mut EventCx, a: GridAction) {
        super::send(cx, self.on.make(a));
    }
}

impl<Msg: Send + 'static> Widget for Strip<Msg> {
    fn name(&self) -> &'static str {
        "koyomi-all-day"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width, self.data.height()))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let size = cx.size();
        let geo = self.geo(size);
        cx.draw_text_in(
            "all-day",
            &mono(9.5, t.faint()),
            Rect::new(2.0, 4.0, GUTTER - 8.0, 4.0 + LANE - 2.0),
            Align::End,
        );
        for i in 0..geo.n {
            let x = geo.col_x(i).0;
            cx.canvas.fill_rect(Rect::new(x, 0.0, x + 1.0, b.y1), t.line());
        }
        for bar in self.data.bars.iter().filter(|bar| bar.lane < self.data.lanes) {
            let r = self.bar_rect(size, bar);
            let dye = colors::dye(t, bar.dye);
            let (fill, fg) = if bar.selected {
                (dye, colors::on_dye(t, dye))
            } else {
                (colors::tint(t, dye).mix(dye, 0.25), colors::on_tint(t, dye))
            };
            cx.canvas.fill_rect(r, fill);
            if !bar.before {
                cx.canvas.fill_rect(
                    Rect::new(r.x0, r.y0, r.x0 + 3.0, r.y1),
                    if bar.selected { fg.with_alpha(0.55) } else { dye },
                );
            }
            let mut spec = ui(12.0, 600.0, fg);
            spec.strikethrough = bar.cancelled;
            let label = if bar.before { format!("‹ {}", bar.title) } else { bar.title.to_string() };
            cx.draw_text_in(&label, &spec, Rect::new(r.x0 + 7.0, r.y0, r.x1 - 4.0, r.y1), Align::Start);
        }
        for (i, n) in self.data.more.iter().enumerate() {
            if *n > 0 {
                let (x0, x1) = geo.col_x(i);
                let y = 4.0 + (self.data.lanes.saturating_sub(1)) as f64 * LANE;
                cx.draw_text_in(
                    &format!("+{n} more"),
                    &mono(10.5, t.text3()),
                    Rect::new(x0 + 8.0, y, x1 - 4.0, y + LANE - 2.0),
                    Align::Start,
                );
            }
        }
        draw::bottom(cx.canvas, b, t.line2(), 1.0);
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        if let Some(g) = cx.state::<StripState>().and_then(|s| s.ghost) {
            let r = self.ghost_rect(cx.size(), g);
            cx.canvas.fill_rect(r, t.focus().with_alpha(0.25));
            draw::border(cx.canvas, r, t.focus(), 1.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&*self.data);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let size = cx.size();
        let geo = self.geo(size);
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, clicks, .. } => {
                let hit = self.hit(size, *pos);
                match (hit, *clicks >= 2) {
                    (Some(i), true) => self.emit(cx, GridAction::Open(self.data.bars[i].key)),
                    (Some(i), false) => {
                        cx.state::<StripState>().press = Some((*pos, Some(i)));
                        cx.capture_pointer();
                        if !self.data.bars[i].selected {
                            self.emit(cx, GridAction::Select(Some(self.data.bars[i].key)));
                        }
                    }
                    (None, true) => {
                        if let Some(c) = geo.col_at(pos.x) {
                            let d = self.data.days[c];
                            let start = d.to_datetime(Time::midnight());
                            let end = d.tomorrow().unwrap_or(d).to_datetime(Time::midnight());
                            self.emit(cx, GridAction::Create { start, end, all_day: true });
                        }
                    }
                    (None, false) => {
                        if let Some(c) = geo.col_at(pos.x) {
                            self.emit(cx, GridAction::Day(self.data.days[c]));
                        }
                    }
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let Some((origin, Some(i))) = cx.state::<StripState>().press else { return Status::Ignored };
                if (*pos - origin).hypot() < SLOP && cx.state::<StripState>().ghost.is_none() {
                    return Status::Handled;
                }
                let bar = &self.data.bars[i];
                let (Some(c0), Some(c1)) = (geo.col_at(origin.x.max(GUTTER)), geo.col_at(pos.x.max(GUTTER))) else {
                    return Status::Handled;
                };
                let shift = c1 as i64 - c0 as i64;
                let first = (bar.first as i64 + shift).clamp(0, geo.n as i64 - 1) as usize;
                let last = (bar.last as i64 + shift).clamp(0, geo.n as i64 - 1) as usize;
                let g = Some((first, last.max(first)));
                let old = cx.state::<StripState>().ghost;
                if old != g {
                    if let Some(o) = old {
                        cx.invalidate_rect(self.ghost_rect(size, o).inflate(2.0, 2.0));
                    }
                    if let Some(n) = g {
                        cx.invalidate_rect(self.ghost_rect(size, n).inflate(2.0, 2.0));
                    }
                    cx.state::<StripState>().ghost = g;
                }
                Status::Handled
            }
            Event::PointerUp { pos, .. } => {
                let st = std::mem::take(cx.state::<StripState>());
                cx.release_pointer();
                if let Some(o) = st.ghost {
                    cx.invalidate_rect(self.ghost_rect(size, o).inflate(2.0, 2.0));
                }
                if let (Some((origin, Some(i))), Some(_)) = (st.press, st.ghost) {
                    let bar = &self.data.bars[i];
                    if let (Some(c0), Some(c1)) = (geo.col_at(origin.x.max(GUTTER)), geo.col_at(pos.x.max(GUTTER))) {
                        let shift = c1 as i64 - c0 as i64;
                        if shift != 0 {
                            let start = crate::rrule::add_days(bar.start, shift).unwrap_or(bar.start);
                            let end = crate::rrule::add_days(start, bar.days.max(1)).unwrap_or(start);
                            self.emit(
                                cx,
                                GridAction::Move {
                                    key: bar.key,
                                    start: start.to_datetime(Time::midnight()),
                                    end: end.to_datetime(Time::midnight()),
                                    all_day: true,
                                },
                            );
                        }
                    }
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Row, label: Some("All-day events".into()), ..Default::default() })
    }
}
