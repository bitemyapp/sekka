//! The month view: square cells on a 1pt `line` grid, day numbers in
//! mono, today on the focus colour, events as chips coloured by their
//! calendar (all-day and multi-day ones as bars across their days).
//!
//! The grid draws the lines, the weekday names and the week numbers;
//! each day is a child that draws its number and chips, so selecting a
//! day or an event redraws two cells. The grid takes the pointer: a
//! chip pressed selects its event, dragged moves it to another day;
//! double-clicking a day makes an all-day event there.

use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::draw;
use aizome_kit::element::{Element, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Length, Point, Rect, Size};
use aizome_kit::sidestep_ui::{Color, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use aizome_kit::{Align, Layoutable};
use jiff::civil::{Date, DateTime, Time};

use crate::model::Dye;
use crate::occur::Occurrence;
use crate::ui::colors;
use crate::ui::week::GridAction;

pub const HEADER: f64 = 24.0;
pub const WEEK_NUMBERS: f64 = 30.0;
/// A chip's lane in a cell.
pub const CHIP: f64 = 18.0;
/// Where the first lane starts in a cell.
const TOP: f64 = 24.0;

#[derive(Clone, Debug, PartialEq, Hash)]
pub enum ChipKind {
    /// An all-day or multi-day event: a bar across its days. `seg` is
    /// the day's place along the bar in its week (the title runs on
    /// from the first).
    Bar {
        seg: u8,
        before: bool,
        after: bool,
    },
    Timed {
        time: Arc<str>,
    },
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Chip {
    pub key: u64,
    pub kind: ChipKind,
    pub title: Arc<str>,
    pub dye: Dye,
    pub selected: bool,
    pub cancelled: bool,
    pub all_day: bool,
    /// The event's real start and end, for moving it.
    pub start: DateTime,
    pub end: DateTime,
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct DayData {
    pub date: Date,
    pub in_month: bool,
    pub today: bool,
    pub selected: bool,
    pub weekend: bool,
    /// Chips by lane (a lane a bar from another day holds is empty here).
    pub lanes: Vec<Option<Chip>>,
}

impl DayData {
    /// How many chips show in `lanes_fit` lanes, and how many more there are.
    pub fn shown(&self, lanes_fit: usize) -> (usize, usize) {
        let total = self.lanes.iter().filter(|c| c.is_some()).count();
        if self.lanes.len() <= lanes_fit {
            return (self.lanes.len(), 0);
        }
        let room = lanes_fit.saturating_sub(1);
        let showing = self.lanes[..room].iter().filter(|c| c.is_some()).count();
        (room, total - showing)
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct MonthData {
    pub weeks: Vec<(u32, Vec<Rc<DayData>>)>,
    pub names: Vec<String>,
    pub week_numbers: bool,
}

/// Lay a month's (or any run of weeks') occurrences into cells: bars in
/// lanes that hold across each week, timed events in the lanes left.
pub fn build(
    weeks: &[Vec<Date>],
    occ: &[Occurrence],
    chip: impl Fn(&Occurrence) -> (u64, Arc<str>, Dye, bool, Arc<str>),
    month: i8,
    today: Date,
    cursor: Date,
    weekend: impl Fn(Date) -> bool,
) -> Vec<Vec<DayData>> {
    let mut out = Vec::new();
    for days in weeks {
        let (first, last) = (days[0], days[days.len() - 1]);
        let mut cells: Vec<DayData> = days
            .iter()
            .map(|d| DayData {
                date: *d,
                in_month: d.month() == month,
                today: *d == today,
                selected: *d == cursor,
                weekend: weekend(*d),
                lanes: Vec::new(),
            })
            .collect();
        // Bars first, longest first, each in the lowest lane free on all
        // its days.
        let mut bars: Vec<&Occurrence> =
            occ.iter().filter(|o| o.spans_days() && o.first_day() <= last && o.last_day() >= first).collect();
        bars.sort_by(|a, b| a.first_day().cmp(&b.first_day()).then((b.last_day(), b.end).cmp(&(a.last_day(), a.end))));
        for o in bars {
            let (key, title, dye, selected, _) = chip(o);
            let a = days.iter().position(|d| *d >= o.first_day()).unwrap_or(0);
            let z = days.iter().rposition(|d| *d <= o.last_day()).unwrap_or(days.len() - 1);
            let lane = (0..).find(|l| (a..=z).all(|i| cells[i].lanes.get(*l).is_none_or(|c| c.is_none()))).unwrap_or(0);
            for (i, cell) in cells.iter_mut().enumerate().take(z + 1).skip(a) {
                if cell.lanes.len() <= lane {
                    cell.lanes.resize(lane + 1, None);
                }
                cell.lanes[lane] = Some(Chip {
                    key,
                    kind: ChipKind::Bar {
                        seg: (i - a) as u8,
                        before: i > a || o.first_day() < days[a],
                        after: i < z || o.last_day() > days[z],
                    },
                    title: title.clone(),
                    dye,
                    selected,
                    cancelled: o.cancelled,
                    all_day: o.all_day,
                    start: o.start,
                    end: o.end,
                });
            }
        }
        // Then each day's timed events, in the free lanes, in order.
        for o in occ.iter().filter(|o| !o.spans_days()) {
            let Some(i) = days.iter().position(|d| *d == o.first_day()) else { continue };
            let (key, title, dye, selected, time) = chip(o);
            let cell = &mut cells[i];
            let lane = (0..).find(|l| cell.lanes.get(*l).is_none_or(|c| c.is_none())).unwrap_or(0);
            if cell.lanes.len() <= lane {
                cell.lanes.resize(lane + 1, None);
            }
            cell.lanes[lane] = Some(Chip {
                key,
                kind: ChipKind::Timed { time },
                title,
                dye,
                selected,
                cancelled: o.cancelled,
                all_day: false,
                start: o.start,
                end: o.end,
            });
        }
        out.push(cells);
    }
    out
}

/// Where things are in the month grid.
#[derive(Clone, Copy, Debug)]
struct Geo {
    width: f64,
    height: f64,
    rows: usize,
    gutter: f64,
}

impl Geo {
    fn col_x(&self, c: usize) -> (f64, f64) {
        let w = (self.width - self.gutter) / 7.0;
        ((self.gutter + w * c as f64).round(), (self.gutter + w * (c + 1) as f64).round())
    }

    fn row_y(&self, r: usize) -> (f64, f64) {
        let h = (self.height - HEADER) / self.rows.max(1) as f64;
        ((HEADER + h * r as f64).round(), (HEADER + h * (r + 1) as f64).round())
    }

    /// A cell's inside (right of its left line, below its top line).
    fn cell(&self, r: usize, c: usize) -> Rect {
        let (x0, x1) = self.col_x(c);
        let (y0, y1) = self.row_y(r);
        Rect::new(x0 + 1.0, y0 + 1.0, x1, y1)
    }

    fn at(&self, p: Point) -> Option<(usize, usize)> {
        if p.y < HEADER || p.x < self.gutter {
            return None;
        }
        let c = (0..7).find(|c| p.x < self.col_x(*c).1).unwrap_or(6);
        let r = (0..self.rows).find(|r| p.y < self.row_y(*r).1).unwrap_or(self.rows.saturating_sub(1));
        Some((r, c))
    }

    fn lanes_fit(&self, cell: Rect) -> usize {
        ((cell.height() - TOP - 2.0) / CHIP).floor().max(1.0) as usize
    }
}

fn lane_rect(cell: Rect, lane: usize) -> Rect {
    let y = cell.y0 + TOP + lane as f64 * CHIP;
    Rect::new(cell.x0, y, cell.x1, y + CHIP - 2.0)
}

fn mono(size: f64, color: Color) -> TextSpec {
    TextSpec::new(FontSpec { tabular: true, ..FontSpec::mono(size) }, color)
}

fn ui(size: f64, weight: f32, color: Color) -> TextSpec {
    let mut f = FontSpec::ui(size);
    f.weight = weight;
    TextSpec::new(f, color)
}

/// A chip and the cell (row, column) it's in.
type ChipAt = (Chip, (usize, usize));

#[derive(Default)]
struct DragState {
    press: Option<(Point, Chip, (usize, usize))>,
    /// The cell the chip would land in, and where the pointer is.
    target: Option<(usize, usize)>,
    at: Option<Point>,
}

struct MonthGrid<Msg> {
    data: Rc<MonthData>,
    on: MsgMap<GridAction>,
    geo: Geo,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

pub fn grid<Msg: Send + 'static>(data: Rc<MonthData>, on: impl Fn(GridAction) -> Msg + 'static) -> Element<Msg> {
    let children: Vec<Element<Msg>> = data
        .weeks
        .iter()
        .flat_map(|(_, days)| days.iter().map(|d| Element::new(Cell { d: d.clone() }, Vec::new()).key(("day", d.date))))
        .collect();
    let gutter = if data.week_numbers { WEEK_NUMBERS } else { 0.0 };
    Element::new(
        MonthGrid::<Msg> {
            geo: Geo { width: 0.0, height: 0.0, rows: data.weeks.len(), gutter },
            data,
            on: MsgMap::new(on),
            _m: std::marker::PhantomData,
        },
        children,
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .id("month-grid")
}

impl<Msg: Send + 'static> MonthGrid<Msg> {
    fn day(&self, r: usize, c: usize) -> Option<&Rc<DayData>> {
        self.data.weeks.get(r).and_then(|(_, d)| d.get(c))
    }

    /// The chip under `p` (and its cell), or the "+N more" (none, with
    /// `true`).
    fn chip_at(&self, p: Point) -> (Option<ChipAt>, bool) {
        let Some((r, c)) = self.geo.at(p) else { return (None, false) };
        let Some(d) = self.day(r, c) else { return (None, false) };
        let cell = self.geo.cell(r, c);
        let fit = self.geo.lanes_fit(cell);
        let (shown, more) = d.shown(fit);
        if p.y < cell.y0 + TOP {
            return (None, false);
        }
        let lane = ((p.y - cell.y0 - TOP) / CHIP) as usize;
        if more > 0 && lane == shown {
            return (None, true);
        }
        match d.lanes.get(lane) {
            Some(Some(chip)) if lane < shown => (Some((chip.clone(), (r, c))), false),
            _ => (None, false),
        }
    }

    fn target_rect(&self, t: (usize, usize)) -> Rect {
        self.geo.cell(t.0, t.1).inflate(2.0, 2.0)
    }

    fn ghost_rect(&self, p: Point) -> Rect {
        Rect::new(p.x - 8.0, p.y - 9.0, p.x + 152.0, p.y + 9.0).inflate(4.0, 4.0)
    }

    fn emit(&self, cx: &mut EventCx, a: GridAction) {
        super::send(cx, self.on.make(a));
    }
}

impl<Msg: Send + 'static> Widget for MonthGrid<Msg> {
    fn name(&self) -> &'static str {
        "koyomi-month"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 800.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 600.0 },
        );
        self.geo = Geo { width: size.width, height: size.height, rows: self.data.weeks.len(), gutter: self.geo.gutter };
        let mut i = 0;
        for r in 0..self.data.weeks.len() {
            for c in 0..self.data.weeks[r].1.len() {
                let cell = self.geo.cell(r, c);
                cx.layout_child(i, Constraints::tight(cell.size()));
                cx.place_child(i, cell.origin());
                i += 1;
            }
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        let geo = self.geo;
        for (c, name) in self.data.names.iter().enumerate() {
            let (x0, x1) = geo.col_x(c);
            let mut cap = mono(10.5, t.text3());
            cap.letter_spacing = 1.2;
            cx.draw_text_in(name, &cap, Rect::new(x0 + 8.0, 0.0, x1 - 4.0, HEADER), Align::Start);
        }
        // The grid: 1pt lines.
        for c in 0..7 {
            let x = geo.col_x(c).0;
            cx.canvas.fill_rect(Rect::new(x, HEADER, x + 1.0, b.y1), t.line());
        }
        for r in 0..geo.rows {
            let y = geo.row_y(r).0;
            cx.canvas.fill_rect(Rect::new(0.0, y, b.x1, y + 1.0), t.line());
            if self.data.week_numbers {
                let (y0, _) = geo.row_y(r);
                let cell = Rect::new(0.0, y0 + 1.0, geo.gutter, geo.row_y(r).1);
                cx.canvas.fill_rect(cell, t.surface().mix(t.sunk(), 0.5));
                let n = self.data.weeks[r].0;
                cx.draw_text_in(
                    &n.to_string(),
                    &mono(10.5, t.faint()),
                    Rect::new(0.0, y0 + 4.0, geo.gutter - 6.0, y0 + 20.0),
                    Align::End,
                );
            }
        }
        draw::bottom(cx.canvas, Rect::new(0.0, 0.0, b.x1, HEADER), t.line(), 1.0);
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let Some(st) = cx.state::<DragState>() else { return };
        let (Some(target), Some(p), Some((_, chip, _))) = (st.target, st.at, st.press.as_ref()) else { return };
        let r = self.geo.cell(target.0, target.1);
        cx.canvas.fill_rect(r, t.focus().with_alpha(0.12));
        draw::border(cx.canvas, r, t.focus(), 2.0);
        let g = Rect::new(p.x - 8.0, p.y - 9.0, p.x + 152.0, p.y + 9.0);
        let dye = colors::dye(t, chip.dye);
        if t.shadows() {
            cx.canvas.fill_rect(g + aizome_kit::Vec2::new(3.0, 3.0), t.slab());
        }
        cx.canvas.fill_rect(g, dye);
        cx.draw_text_in(
            &chip.title,
            &ui(11.5, 600.0, colors::on_dye(t, dye)),
            Rect::new(g.x0 + 6.0, g.y0, g.x1 - 4.0, g.y1),
            Align::Start,
        );
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.data.names).add(&self.data.week_numbers).add(&self.data.weeks.len());
        for (n, _) in &self.data.weeks {
            h.add(n);
        }
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

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, clicks, .. } => {
                let p = *pos;
                if p.x < self.geo.gutter && p.y >= HEADER {
                    let r = (0..self.geo.rows).find(|r| p.y < self.geo.row_y(*r).1).unwrap_or(0);
                    if let Some(d) = self.day(r, 0) {
                        let date = d.date;
                        self.emit(cx, GridAction::ShowWeek(date));
                    }
                    return Status::Handled;
                }
                let Some((r, c)) = self.geo.at(p) else { return Status::Handled };
                let Some(day) = self.day(r, c).map(|d| d.date) else { return Status::Handled };
                match (self.chip_at(p), *clicks >= 2) {
                    ((Some((chip, _)), _), true) => self.emit(cx, GridAction::Open(chip.key)),
                    ((Some((chip, cell)), _), false) => {
                        let selected = chip.selected;
                        let key = chip.key;
                        cx.state::<DragState>().press = Some((p, chip, cell));
                        cx.capture_pointer();
                        if !selected {
                            self.emit(cx, GridAction::Select(Some(key)));
                        }
                    }
                    ((None, true), _) => self.emit(cx, GridAction::ShowDay(day)),
                    ((None, false), true) => {
                        let start = day.to_datetime(Time::midnight());
                        let end = day.tomorrow().unwrap_or(day).to_datetime(Time::midnight());
                        self.emit(cx, GridAction::Create { start, end, all_day: true });
                    }
                    ((None, false), false) => self.emit(cx, GridAction::Day(day)),
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let Some((origin, _, home)) = cx.state::<DragState>().press.clone() else { return Status::Ignored };
                let st = cx.state::<DragState>();
                if st.at.is_none() && (*pos - origin).hypot() < 4.0 {
                    return Status::Handled;
                }
                let target = self.geo.at(*pos).or(Some(home));
                let (old_t, old_p) = (st.target, st.at);
                st.target = target;
                st.at = Some(*pos);
                if old_t != target {
                    if let Some(o) = old_t {
                        cx.invalidate_rect(self.target_rect(o));
                    }
                    if let Some(n) = target {
                        cx.invalidate_rect(self.target_rect(n));
                    }
                }
                if let Some(o) = old_p {
                    cx.invalidate_rect(self.ghost_rect(o));
                }
                cx.invalidate_rect(self.ghost_rect(*pos));
                Status::Handled
            }
            Event::PointerUp { .. } => {
                let st = std::mem::take(cx.state::<DragState>());
                cx.release_pointer();
                if let Some(o) = st.target {
                    cx.invalidate_rect(self.target_rect(o));
                }
                if let Some(o) = st.at {
                    cx.invalidate_rect(self.ghost_rect(o));
                }
                if let (Some((_, chip, home)), Some(target)) = (st.press, st.target)
                    && target != home
                    && let (Some(from), Some(to)) =
                        (self.day(home.0, home.1).map(|d| d.date), self.day(target.0, target.1).map(|d| d.date))
                {
                    let shift = crate::model::days_between(from, to);
                    let mv = |dt: DateTime| dt.checked_add(jiff::Span::new().days(shift)).unwrap_or(dt);
                    self.emit(
                        cx,
                        GridAction::Move {
                            key: chip.key,
                            start: mv(chip.start),
                            end: mv(chip.end),
                            all_day: chip.all_day,
                        },
                    );
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Table, label: Some("Month".into()), ..Default::default() })
    }
}

/// A day of the month grid.
struct Cell {
    d: Rc<DayData>,
}

impl Widget for Cell {
    fn name(&self) -> &'static str {
        "koyomi-day"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.min
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        let d = &self.d;
        // The window's ground is the surface already, and a month redraws
        // nearly whole when it changes: days fill nothing more of their own
        // (other months' days are told by their faint numbers), so a month
        // costs one pass over its pixels.
        if d.selected {
            cx.canvas.fill_rect(r, t.selection().with_alpha(if t.is_dark() { 0.16 } else { 0.1 }));
        }
        // The day's number, top left; the first of a month says which.
        let label = if d.date.day() == 1 {
            format!("{} {}", d.date.day(), crate::format::month_short(d.date.month()).to_uppercase())
        } else {
            d.date.day().to_string()
        };
        let color = if d.today {
            t.on_focus()
        } else if d.in_month {
            t.text()
        } else {
            t.faint()
        };
        let spec = TextSpec::new(
            FontSpec { weight: if d.today { 600.0 } else { 400.0 }, tabular: true, ..FontSpec::mono(12.0) },
            color,
        );
        let w = cx.layout_text(&label, &spec, None).size().width.ceil();
        let num = Rect::new(4.0, 3.0, 4.0 + w + 8.0, 20.0);
        if d.today {
            cx.canvas.fill_rect(num, t.focus());
        }
        cx.draw_text_in(&label, &spec, num, Align::Center);
        let fit = ((r.height() - TOP - 2.0) / CHIP).floor().max(1.0) as usize;
        let (shown, more) = d.shown(fit);
        for (lane, chip) in d.lanes.iter().enumerate().take(shown) {
            let Some(chip) = chip else { continue };
            let lr = lane_rect(r, lane);
            paint_chip(cx, chip, lr);
        }
        if more > 0 {
            let lr = lane_rect(r, shown);
            cx.draw_text_in(
                &format!("+{more} more"),
                &mono(10.5, t.text3()),
                Rect::new(lr.x0 + 6.0, lr.y0, lr.x1 - 4.0, lr.y1),
                Align::Start,
            );
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&*self.d);
    }

    fn overflow(&self) -> f64 {
        // Bars run over the line on their left.
        1.0
    }

    fn semantics(&self) -> Option<Semantics> {
        let n = self.d.lanes.iter().filter(|c| c.is_some()).count();
        Some(Semantics {
            role: SemanticRole::Cell,
            label: Some(format!(
                "{}, {} event{}",
                crate::format::long_date(self.d.date),
                n,
                if n == 1 { "" } else { "s" }
            )),
            selected: Some(self.d.selected),
            ..Default::default()
        })
    }
}

fn paint_chip(cx: &mut PaintCx, chip: &Chip, lr: Rect) {
    let t = cx.theme();
    let dye = colors::dye(t, chip.dye);
    match &chip.kind {
        ChipKind::Bar { seg, before, after } => {
            // Bars run through the grid's lines to the next day.
            let x0 = if *before { lr.x0 - 1.0 } else { lr.x0 + 3.0 };
            let x1 = if *after { lr.x1 } else { lr.x1 - 3.0 };
            let bar = Rect::new(x0, lr.y0, x1, lr.y1);
            let (fill, fg) = if chip.selected {
                (dye, colors::on_dye(t, dye))
            } else {
                (colors::tint(t, dye).mix(dye, 0.3), colors::on_tint(t, dye))
            };
            cx.canvas.fill_rect(bar, fill);
            if !before {
                cx.canvas.fill_rect(
                    Rect::new(bar.x0, bar.y0, bar.x0 + 3.0, bar.y1),
                    if chip.selected { fg.with_alpha(0.5) } else { dye },
                );
            }
            // The title runs on across the bar's days: each day draws its
            // stretch of it, as if from where the bar starts.
            let mut spec = ui(11.5, 600.0, fg);
            spec.strikethrough = chip.cancelled;
            let day = lr.width() + 1.0;
            let x = lr.x0 + 7.0 - f64::from(*seg) * day;
            let layout = cx.layout_text(&chip.title, &spec, None);
            let h = layout.size().height;
            if x + layout.size().width > bar.x0 {
                cx.canvas.save();
                cx.canvas.clip_rect(Rect::new(bar.x0, bar.y0, bar.x1 - if *after { 0.0 } else { 3.0 }, bar.y1));
                cx.canvas.draw_text(&layout, Point::new(x.round(), (bar.y0 + (bar.height() - h) / 2.0).round()));
                cx.canvas.restore();
            }
        }
        ChipKind::Timed { time } => {
            let inner = Rect::new(lr.x0 + 3.0, lr.y0, lr.x1 - 3.0, lr.y1);
            let (fg, sub) = if chip.selected {
                cx.canvas.fill_rect(inner, dye);
                let on = colors::on_dye(t, dye);
                (on, on.with_alpha(0.8))
            } else {
                (if chip.cancelled { t.faint() } else { t.text() }, t.text3())
            };
            if !chip.selected {
                cx.canvas.fill_rect(Rect::new(inner.x0 + 3.0, inner.y0 + 5.0, inner.x0 + 9.0, inner.y0 + 11.0), dye);
            }
            let x = inner.x0 + 13.0;
            let tw =
                cx.draw_text_in(time, &mono(10.5, sub), Rect::new(x, inner.y0, inner.x1 - 2.0, inner.y1), Align::Start);
            let mut spec = ui(11.5, 500.0, fg);
            spec.strikethrough = chip.cancelled;
            cx.draw_text_in(
                &chip.title,
                &spec,
                Rect::new(x + tw + 5.0, inner.y0, inner.x1 - 2.0, inner.y1),
                Align::Start,
            );
        }
    }
}
