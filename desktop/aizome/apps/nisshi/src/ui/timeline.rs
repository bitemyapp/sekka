//! The timeline: how many messages over the boot, errors in red and
//! warnings in amber at the foot of each bar, the rest above; the time
//! chosen shaded in the focus colour and the message selected marked.
//! Dragging across it chooses a stretch of time; a press goes to the
//! message nearest that moment.

use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::widget::Event;
use aizome_kit::{column, row};

use crate::app::{Brush, Msg, Nisshi};
use crate::model::Histogram;

/// What the timeline draws: fractions of its width in ten-thousandths.
#[derive(Clone, Hash)]
struct State {
    h: Arc<Histogram>,
    range: Option<(u32, u32)>,
    brush: Option<(u32, u32)>,
    mark: Option<u32>,
}

fn part(f: f64) -> u32 {
    (f.clamp(0.0, 1.0) * 10_000.0).round() as u32
}

pub const HEIGHT: f64 = 38.0;

pub fn view(n: &Nisshi) -> Element<Msg> {
    let h = n.view.histogram.clone();
    if h.buckets.is_empty() || n.in_file() && h.to - h.from < 2_000_000 {
        return space(0.0);
    }
    let state = State {
        range: n.filter.range().map(|(a, b)| (part(h.fraction(a)), part(h.fraction(b)))),
        brush: n.brush.map(|(a, b)| (part(a), part(b))),
        mark: n.selection().filter(|e| e.time > 0).map(|e| part(h.fraction(e.time))),
        h: h.clone(),
    };
    let chart = canvas(state, draw)
        .on_event(|e, size, s: &State| {
            let at = |x: f64| (x / size.width.max(1.0)).clamp(0.0, 1.0);
            match e {
                Event::PointerDown { pos, button: PointerButton::Left, .. } => Some(Msg::Brush(Brush::Down(at(pos.x)))),
                Event::PointerMove { pos, .. } if s.brush.is_some() => Some(Msg::Brush(Brush::Move(at(pos.x)))),
                Event::PointerUp { pos, button: PointerButton::Left, .. } if s.brush.is_some() => {
                    Some(Msg::Brush(Brush::Up(at(pos.x))))
                }
                _ => None,
            }
        })
        .width(Length::Fill)
        .height(HEIGHT)
        .label("Messages over time: drag to choose a stretch of time")
        .tooltip("Drag to show a stretch of time; press to go to a moment")
        .id("timeline");
    let label = |t: i64| text(n.clock.short(t)).size(10.5).tabular().role(Role::Text3);
    let middle = h.from + (h.to - h.from) / 2;
    let scale = row![label(h.from), spacer(), label(middle), spacer(), label(h.to)].width(Length::Fill);
    container(column![chart, scale].spacing(2.0).width(Length::Fill))
        .padding([6.0, 10.0, 4.0, 10.0])
        .fill(Tone::Chrome)
        .edges(Edge::Line, Sides::BOTTOM)
        .width(Length::Fill)
        .into()
}

fn draw(c: &mut aizome_kit::sidestep_ui::Canvas, b: Rect, s: &State, t: &Theme) {
    c.fill_rect(b, t.sunk());
    let p = &t.palette;
    let (red, amber) = (aizome_kit::theme::color(p.red), aizome_kit::theme::color(p.amber));
    let rest = t.text3().with_alpha(0.55);
    let n = s.h.buckets.len().max(1);
    let tallest = (s.h.tallest().max(1) as f64).sqrt();
    let inner = Rect::new(b.x0 + 1.0, b.y0 + 2.0, b.x1 - 1.0, b.y1);
    let w = inner.width() / n as f64;
    let x_of = |f: u32| inner.x0 + inner.width() * f64::from(f) / 10_000.0;
    for (i, bucket) in s.h.buckets.iter().enumerate() {
        let total = bucket[0] + bucket[1] + bucket[2];
        if total == 0 {
            continue;
        }
        // A square root's scale: a few errors still show beside thousands.
        let height = (f64::from(total).sqrt() / tallest * inner.height()).max(1.0);
        let x0 = (inner.x0 + i as f64 * w).floor();
        let x1 = (inner.x0 + (i + 1) as f64 * w).floor().max(x0 + 1.0);
        let mut y = inner.y1;
        let bar = |c: &mut aizome_kit::sidestep_ui::Canvas, y: &mut f64, k: u32, color: Color| {
            if k == 0 {
                return;
            }
            let h = (height * f64::from(k) / f64::from(total)).max(1.0);
            c.fill_rect(Rect::new(x0, (*y - h).round(), x1, y.round()), color);
            *y -= h;
        };
        bar(c, &mut y, bucket[0], red);
        bar(c, &mut y, bucket[1], amber);
        bar(c, &mut y, bucket[2], rest);
    }
    let focus = t.focus();
    let shade = |c: &mut aizome_kit::sidestep_ui::Canvas, (a, z): (u32, u32), color: Color| {
        let (a, z) = (a.min(z), a.max(z));
        let (x0, x1) = (x_of(a).floor(), x_of(z).ceil().max(x_of(a).floor() + 1.0));
        c.fill_rect(Rect::new(x0, b.y0, x1, b.y1), color.with_alpha(0.16));
        c.fill_rect(Rect::new(x0, b.y0, x0 + 1.0, b.y1), color);
        c.fill_rect(Rect::new(x1 - 1.0, b.y0, x1, b.y1), color);
    };
    if let Some(r) = s.range {
        shade(c, r, focus);
    }
    if let Some(r) = s.brush {
        shade(c, r, t.selection());
    }
    if let Some(m) = s.mark {
        let x = x_of(m).round();
        c.fill_rect(Rect::new(x, b.y0, x + 1.0, b.y1), focus);
    }
}
