//! Graphs over time: the last two minutes of a figure as filled areas,
//! the newest reading at the right. Drawn only when a reading changes
//! them.

use aizome_kit::Color;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Rect, Size};
use aizome_kit::sidestep_ui::kurbo::{BezPath, Shape};
use aizome_kit::sidestep_ui::{LineJoin, StrokeStyle};
use aizome_kit::theme::Theme;
use aizome_kit::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};
use aizome_kit::{Element, Layoutable, Length};

use crate::history::{LEN, Ring};

/// A series' colour, from the palette's accents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hue {
    Blue,
    Violet,
    Cyan,
    Green,
    Amber,
}

impl Hue {
    pub fn color(self, t: &Theme) -> Color {
        let p = &t.palette;
        aizome_kit::theme::color(match self {
            Hue::Blue => p.blue,
            Hue::Violet => p.violet,
            Hue::Cyan => p.cyan,
            Hue::Green => p.green,
            Hue::Amber => p.amber,
        })
    }
}

/// How the two series show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    /// The second on top of the first (CPU user and system).
    Stacked,
    /// Each from the bottom, the second as a line over the first's area
    /// (memory used and cached).
    Over,
    /// The first upward from the middle, the second downward (read and
    /// write, in and out).
    Mirrored,
}

/// The top of the scale: fixed, or the series' peak.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    Fixed(f32),
    Auto,
}

pub fn graph<Msg: 'static>(
    series: [Ring; 2],
    hues: [Hue; 2],
    style: Style,
    scale: Scale,
    label: String,
) -> Element<Msg> {
    Element::new(Graph { series, hues, style, scale }, Vec::new()).width(Length::Fill).height(64.0).label(label)
}

struct Graph {
    series: [Ring; 2],
    hues: [Hue; 2],
    style: Style,
    scale: Scale,
}

/// A round top for an automatic scale: 1, 2 or 5 times a power of ten.
pub fn nice(v: f32) -> f32 {
    if v <= 0.0 {
        return 1.0;
    }
    let p = 10f32.powf(v.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if v <= m * p {
            return m * p;
        }
    }
    10.0 * p
}

impl Graph {
    fn top(&self) -> f32 {
        match self.scale {
            Scale::Fixed(m) => m,
            Scale::Auto => {
                let m = match self.style {
                    Style::Stacked => (0..self.series[0].len())
                        .map(|i| self.series[0].get(i) + self.series[1].get(i))
                        .fold(0.0, f32::max),
                    _ => self.series[0].max().max(self.series[1].max()),
                };
                nice(m.max(1.0))
            }
        }
    }
}

impl Widget for Graph {
    fn name(&self) -> &'static str {
        "myaku-graph"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(if bc.max.width.is_finite() { bc.max.width } else { 200.0 }, 64.0))
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.series).add(&self.hues).add(&self.style).add(&self.top().to_bits());
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme().clone();
        let b = cx.bounds();
        cx.canvas.fill_rect(b, t.sunk());
        let inner = Rect::new(b.x0 + 1.0, b.y0 + 1.0, b.x1 - 1.0, b.y1 - 1.0);
        // Quarters, faintly.
        for q in 1..4 {
            let y = (inner.y0 + inner.height() * f64::from(q) / 4.0).round();
            cx.canvas.fill_rect(Rect::new(inner.x0, y, inner.x1, y + 1.0), t.line().with_alpha(0.55));
        }
        let top = self.top();
        let n = self.series[0].len().max(self.series[1].len());
        if n >= 2 {
            let step = inner.width() / (LEN - 1) as f64;
            let x = |i: usize| inner.x1 - (n - 1 - i) as f64 * step;
            let (mid, half) = match self.style {
                Style::Mirrored => (inner.y0 + inner.height() / 2.0, inner.height() / 2.0),
                _ => (inner.y1, inner.height()),
            };
            let y_up = |v: f32| mid - (f64::from((v / top).clamp(0.0, 1.0)) * half);
            let y_down = |v: f32| mid + (f64::from((v / top).clamp(0.0, 1.0)) * half);
            let area = |lo: &dyn Fn(usize) -> f64, hi: &dyn Fn(usize) -> f64| {
                let mut p = BezPath::new();
                p.move_to((x(0), lo(0)));
                for i in 0..n {
                    p.line_to((x(i), hi(i)));
                }
                for i in (0..n).rev() {
                    p.line_to((x(i), lo(i)));
                }
                p.close_path();
                p
            };
            let line = |f: &dyn Fn(usize) -> f64| {
                let mut p = BezPath::new();
                p.move_to((x(0), f(0)));
                for i in 1..n {
                    p.line_to((x(i), f(i)));
                }
                p
            };
            let stroke = StrokeStyle::new(1.5).with_join(LineJoin::Round);
            let (a, c) = (&self.series[0], &self.series[1]);
            let (ha, hc) = (self.hues[0].color(&t), self.hues[1].color(&t));
            let fill = if t.is_dark() { 0.32 } else { 0.26 };
            cx.canvas.save();
            cx.canvas.clip_rect(inner);
            match self.style {
                Style::Stacked => {
                    let base = |_: usize| mid;
                    let first = |i: usize| y_up(a.get(i));
                    let both = |i: usize| y_up(a.get(i) + c.get(i));
                    cx.canvas.fill(&area(&base, &first), ha.with_alpha(fill));
                    cx.canvas.fill(&area(&first, &both), hc.with_alpha(fill));
                    cx.canvas.stroke(&line(&both), &stroke, hc);
                    cx.canvas.stroke(&line(&first), &stroke, ha);
                }
                Style::Over => {
                    let base = |_: usize| mid;
                    let first = |i: usize| y_up(a.get(i));
                    let second = |i: usize| y_up(a.get(i) + c.get(i));
                    cx.canvas.fill(&area(&base, &first), ha.with_alpha(fill));
                    cx.canvas.fill(&area(&first, &second), hc.with_alpha(fill * 0.45));
                    cx.canvas.stroke(&line(&first), &stroke, ha);
                }
                Style::Mirrored => {
                    let base = |_: usize| mid;
                    let up = |i: usize| y_up(a.get(i));
                    let down = |i: usize| y_down(c.get(i));
                    cx.canvas.fill(&area(&base, &up), ha.with_alpha(fill));
                    cx.canvas.fill(&area(&base, &down), hc.with_alpha(fill));
                    cx.canvas.stroke(&line(&up), &stroke, ha);
                    cx.canvas.stroke(&line(&down), &stroke, hc);
                    cx.canvas.fill_rect(Rect::new(inner.x0, mid.round(), inner.x1, mid.round() + 1.0), t.line2());
                }
            }
            cx.canvas.restore();
        }
        cx.canvas.stroke(&b.inset(-0.5).to_path(0.1), &StrokeStyle::new(1.0), t.line());
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scales_round_up() {
        assert_eq!(super::nice(0.0), 1.0);
        assert_eq!(super::nice(3.2), 5.0);
        assert_eq!(super::nice(170.0), 200.0);
        assert_eq!(super::nice(1_100_000.0), 2_000_000.0);
    }
}
