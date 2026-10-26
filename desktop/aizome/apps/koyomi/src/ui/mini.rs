//! Small months: the sidebar's mini-month (with its own ‹ › to browse)
//! and the year view's twelve.

use aizome_kit::draw;
use aizome_kit::element::{Element, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::icons::Icon;
use aizome_kit::layout::{Constraints, Length, Rect, Size};
use aizome_kit::sidestep_ui::{Color, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Theme};
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};
use aizome_kit::{Align, Layoutable};
use jiff::civil::{Date, Weekday};

use crate::rrule::{add_days, week_start};

/// What a small month shows.
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Mini {
    pub year: i16,
    pub month: i8,
    pub today: Date,
    pub cursor: Date,
    /// Days marked as showing in the main view (a week, a day).
    pub range: Option<(Date, Date)>,
    pub first_day: Weekday,
    /// The days (of the 42 shown) that have events.
    pub busy: u64,
}

impl Mini {
    pub fn first_shown(&self) -> Date {
        let first = Date::new(self.year, self.month, 1).unwrap_or_default();
        week_start(first, self.first_day)
    }

    /// The 42 days shown, from the first.
    pub fn day(&self, i: usize) -> Date {
        add_days(self.first_shown(), i as i64).unwrap_or_default()
    }
}

fn mono(size: f64, color: Color) -> TextSpec {
    TextSpec::new(FontSpec { tabular: true, ..FontSpec::mono(size) }, color)
}

/// Draw a small month's names and days in `r` (6 rows of `row`).
fn paint_days(cx: &mut PaintCx, m: &Mini, r: Rect, row: f64, names_h: f64) {
    let t = cx.theme();
    let w = r.width() / 7.0;
    for i in 0..7 {
        let wd = m.first_day.wrapping_add(i as i64);
        let name = &crate::format::weekday_short(wd)[..1];
        let x0 = (r.x0 + w * i as f64).round();
        cx.draw_text_in(
            name,
            &mono(10.0, t.text3()),
            Rect::new(x0, r.y0, (x0 + w).round(), r.y0 + names_h),
            Align::Center,
        );
    }
    let top = r.y0 + names_h;
    if let Some((a, z)) = m.range {
        // The main view's days, as a band.
        for i in 0..42 {
            let d = m.day(i);
            if d >= a && d <= z {
                let (c, rr) = (i % 7, i / 7);
                let x0 = (r.x0 + w * c as f64).round();
                let y0 = (top + row * rr as f64).round();
                cx.canvas.fill_rect(
                    Rect::new(x0, y0 + 1.0, (r.x0 + w * (c + 1) as f64).round(), y0 + row - 1.0),
                    t.selection().with_alpha(if t.is_dark() { 0.35 } else { 0.18 }),
                );
            }
        }
    }
    for i in 0..42 {
        let d = m.day(i);
        let (c, rr) = (i % 7, i / 7);
        let x0 = (r.x0 + w * c as f64).round();
        let x1 = (r.x0 + w * (c + 1) as f64).round();
        let y0 = (top + row * rr as f64).round();
        let cell = Rect::new(x0, y0, x1, y0 + row);
        let in_month = d.month() == m.month;
        let today = d == m.today;
        let color = if today {
            t.on_focus()
        } else if !in_month {
            t.faint()
        } else {
            t.text()
        };
        if today {
            cx.canvas.fill_rect(draw::centered(cell, (w - 4.0).min(24.0).round(), row - 4.0), t.focus());
        } else if d == m.cursor && in_month {
            draw::border(cx.canvas, draw::centered(cell, (w - 4.0).min(24.0).round(), row - 4.0), t.line2(), 1.0);
        }
        let spec = TextSpec::new(
            FontSpec { weight: if today { 600.0 } else { 400.0 }, tabular: true, ..FontSpec::mono(11.0) },
            color,
        );
        cx.draw_text_in(&d.day().to_string(), &spec, Rect::new(x0, y0, x1, y0 + row - 3.0), Align::Center);
        if m.busy & (1 << i) != 0 && !today {
            let dot = draw::centered(Rect::new(x0, y0 + row - 5.0, x1, y0 + row - 2.0), 3.0, 2.0);
            cx.canvas.fill_rect(dot, if in_month { t.text3() } else { t.faint() });
        }
    }
}

/// What the sidebar's mini-month tells the program.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MiniAction {
    Pick(Date),
    /// ‹ or ›: browse by a month.
    Browse(i32),
}

struct MiniMonth<Msg> {
    m: Mini,
    on: MsgMap<MiniAction>,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

pub const MINI_HEIGHT: f64 = 28.0 + 18.0 + 6.0 * 24.0 + 4.0;

pub fn mini_month<Msg: Send + 'static>(m: Mini, on: impl Fn(MiniAction) -> Msg + 'static) -> Element<Msg> {
    Element::new(MiniMonth::<Msg> { m, on: MsgMap::new(on), _m: std::marker::PhantomData }, Vec::new())
        .width(Length::Fill)
        .height(MINI_HEIGHT)
        .id("mini-month")
}

impl<Msg: Send + 'static> MiniMonth<Msg> {
    fn arrows(&self, size: Size) -> (Rect, Rect) {
        let r = Rect::new(size.width - 28.0, 2.0, size.width - 4.0, 26.0);
        (r - aizome_kit::Vec2::new(24.0, 0.0), r)
    }

    fn days_rect(size: Size) -> Rect {
        Rect::new(0.0, 28.0, size.width, size.height - 4.0)
    }
}

impl<Msg: Send + 'static> Widget for MiniMonth<Msg> {
    fn name(&self) -> &'static str {
        "koyomi-mini-month"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width, MINI_HEIGHT))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let size = cx.size();
        let title = format!("{} {}", crate::format::month_name(self.m.month), self.m.year);
        let mut f = FontSpec::ui(13.0);
        f.weight = 600.0;
        cx.draw_text_in(
            &title,
            &TextSpec::new(f, t.text()),
            Rect::new(6.0, 2.0, size.width - 56.0, 26.0),
            Align::Start,
        );
        let (prev, next) = self.arrows(size);
        Icon::ChevronLeft.draw(cx.canvas, draw::centered(prev, 14.0, 14.0).origin(), 14.0, t.text2());
        Icon::ChevronRight.draw(cx.canvas, draw::centered(next, 14.0, 14.0).origin(), 14.0, t.text2());
        paint_days(cx, &self.m, MiniMonth::<Msg>::days_rect(size), 24.0, 18.0);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.m);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let Event::PointerDown { pos, button: PointerButton::Left, .. } = event else { return Status::Ignored };
        let size = cx.size();
        let (prev, next) = self.arrows(size);
        let a = if prev.contains(*pos) {
            Some(MiniAction::Browse(-1))
        } else if next.contains(*pos) {
            Some(MiniAction::Browse(1))
        } else {
            let r = MiniMonth::<Msg>::days_rect(size);
            let top = r.y0 + 18.0;
            if pos.y >= top && pos.y < top + 6.0 * 24.0 {
                let c = ((pos.x - r.x0) / (r.width() / 7.0)).floor().clamp(0.0, 6.0) as usize;
                let row = ((pos.y - top) / 24.0).floor().clamp(0.0, 5.0) as usize;
                Some(MiniAction::Pick(self.m.day(row * 7 + c)))
            } else {
                None
            }
        };
        if let Some(a) = a {
            super::send(cx, self.on.make(a));
        }
        Status::Handled
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Table,
            label: Some(format!("{} {}", crate::format::month_name(self.m.month), self.m.year)),
            ..Default::default()
        })
    }
}

/// The year view: twelve small months.
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct YearData {
    pub months: Vec<Mini>,
}

struct Year<Msg> {
    data: YearData,
    on: MsgMap<crate::ui::week::GridAction>,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

const Y_TITLE: f64 = 26.0;
const Y_NAMES: f64 = 16.0;
const Y_ROW: f64 = 21.0;
const Y_BLOCK: f64 = Y_TITLE + Y_NAMES + 6.0 * Y_ROW;

pub fn year<Msg: Send + 'static>(
    data: YearData,
    on: impl Fn(crate::ui::week::GridAction) -> Msg + 'static,
) -> Element<Msg> {
    Element::new(Year::<Msg> { data, on: MsgMap::new(on), _m: std::marker::PhantomData }, Vec::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .id("year")
}

impl<Msg: Send + 'static> Year<Msg> {
    /// Each month's block.
    fn blocks(size: Size) -> Vec<Rect> {
        let pad = 16.0;
        let gap_x = 24.0;
        let cols = [6usize, 4, 3, 2]
            .into_iter()
            .find(|c| (size.width - 2.0 * pad - gap_x * (*c as f64 - 1.0)) / *c as f64 >= 180.0)
            .unwrap_or(2);
        let rows = 12_usize.div_ceil(cols);
        let w = ((size.width - 2.0 * pad - gap_x * (cols as f64 - 1.0)) / cols as f64).min(300.0);
        let used = w * cols as f64 + gap_x * (cols as f64 - 1.0);
        let x_start = ((size.width - used) / 2.0).max(pad);
        let gap_y = ((size.height - 2.0 * pad - Y_BLOCK * rows as f64) / (rows as f64 - 1.0).max(1.0)).clamp(8.0, 32.0);
        let used_h = Y_BLOCK * rows as f64 + gap_y * (rows as f64 - 1.0);
        let y_start = ((size.height - used_h) / 2.0).max(pad).min(pad + 8.0);
        (0..12)
            .map(|i| {
                let (c, r) = (i % cols, i / cols);
                let x = (x_start + (w + gap_x) * c as f64).round();
                let y = (y_start + (Y_BLOCK + gap_y) * r as f64).round();
                Rect::new(x, y, (x + w).round(), y + Y_BLOCK)
            })
            .collect()
    }
}

impl<Msg: Send + 'static> Widget for Year<Msg> {
    fn name(&self) -> &'static str {
        "koyomi-year"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.max
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t: &Theme = cx.theme();
        let size = cx.size();
        for (m, r) in self.data.months.iter().zip(Year::<Msg>::blocks(size)) {
            if !cx.canvas.needs(r) {
                continue;
            }
            let current = m.today.year() == m.year && m.today.month() == m.month;
            let mut f = FontSpec::ui(14.0);
            f.weight = 650.0;
            let color = if current { t.role(aizome_kit::Role::Accent) } else { t.text() };
            cx.draw_text_in(
                crate::format::month_name(m.month),
                &TextSpec::new(f, color),
                Rect::new(r.x0 + 4.0, r.y0, r.x1, r.y0 + Y_TITLE - 4.0),
                Align::Start,
            );
            cx.canvas.fill_rect(
                Rect::new(r.x0, r.y0 + Y_TITLE - 3.0, r.x1, r.y0 + Y_TITLE - 2.0),
                if current { t.focus() } else { t.line() },
            );
            paint_days(cx, m, Rect::new(r.x0, r.y0 + Y_TITLE, r.x1, r.y1), Y_ROW, Y_NAMES);
        }
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
        let Event::PointerDown { pos, button: PointerButton::Left, .. } = event else { return Status::Ignored };
        let size = cx.size();
        for (m, r) in self.data.months.iter().zip(Year::<Msg>::blocks(size)) {
            if !r.contains(*pos) {
                continue;
            }
            let first = Date::new(m.year, m.month, 1).unwrap_or_default();
            if pos.y < r.y0 + Y_TITLE {
                super::send(cx, self.on.make(crate::ui::week::GridAction::ShowMonth(first)));
                return Status::Handled;
            }
            let top = r.y0 + Y_TITLE + Y_NAMES;
            if pos.y >= top {
                let c = ((pos.x - r.x0) / (r.width() / 7.0)).floor().clamp(0.0, 6.0) as usize;
                let row = ((pos.y - top) / Y_ROW).floor().clamp(0.0, 5.0) as usize;
                let d = m.day(row * 7 + c);
                super::send(cx, self.on.make(crate::ui::week::GridAction::ShowDay(d)));
            }
            return Status::Handled;
        }
        Status::Ignored
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Table, label: Some("Year".into()), ..Default::default() })
    }
}
