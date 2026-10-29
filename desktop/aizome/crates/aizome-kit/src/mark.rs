//! Item marks: a small tag, glyph or Aizome's mark after a row's label,
//! saying what kind of thing the row is (the toolkit an app is made with,
//! in the Toolchest and the palette).

use crate::icons::Icon;
use crate::layout::{Point, Rect};
use crate::text::{TextCache, TextSpec};
use crate::theme::{FontSpec, Role, Theme, color};
use crate::widget::PaintCx;

/// A mark after a row's label.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemMark {
    /// A short word in a tag, in its colour.
    Tag(String, Role),
    /// A glyph in its colour.
    Glyph(Icon, Role),
    /// Aizome's mark, in its own colours.
    Brand,
}

/// Between the label and the mark, and after it.
pub const MARK_GAP: f64 = 6.0;

impl ItemMark {
    fn tag_spec(size: f64, c: sidestep_ui::Color) -> TextSpec {
        let mut f = FontSpec::mono((size * 0.7 * 2.0).round() / 2.0);
        f.weight = 500.0;
        TextSpec { letter_spacing: 0.2, ..TextSpec::new(f, c) }
    }

    /// The side of a glyph or the brand beside text `size` points, on
    /// whole points.
    fn side(size: f64) -> f64 {
        (size * 0.92).round()
    }

    /// How wide it is beside text `size` points.
    pub fn width(&self, theme: &Theme, text: &mut TextCache, size: f64) -> f64 {
        match self {
            ItemMark::Tag(word, _) => {
                let spec = ItemMark::tag_spec(size, theme.text3());
                (text.layout(theme, word, &spec, None).size().width + 8.0).ceil()
            }
            ItemMark::Glyph(..) => ItemMark::side(size),
            ItemMark::Brand => (ItemMark::side(size) / 4.0).floor() * 4.0,
        }
    }

    /// Draw it from `x`, centred on `cy`, beside text `size` points; `on`
    /// the selection's fill: the width it took.
    pub fn paint(&self, cx: &mut PaintCx, x: f64, cy: f64, size: f64, on: bool) -> f64 {
        let t = cx.theme;
        let x = x.round();
        match self {
            ItemMark::Tag(word, role) => {
                let c = if on { t.on_selection() } else { t.role(*role) };
                let spec = ItemMark::tag_spec(size, c);
                let l = cx.text.layout(t, word, &spec, None);
                let w = (l.size().width + 8.0).ceil();
                let h = (size * 1.08).round();
                let r = Rect::new(x, (cy - h / 2.0).round(), x + w, (cy - h / 2.0).round() + h);
                cx.canvas.fill_rect(r, c.with_alpha(0.12));
                crate::draw::border(cx.canvas, r, c.with_alpha(0.5), 1.0);
                let y = r.y0 + ((h - l.size().height) / 2.0).round();
                cx.canvas.draw_text(&l, Point::new(x + 4.0, y));
                w
            }
            ItemMark::Glyph(icon, role) => {
                let c = if on { t.on_selection() } else { t.role(*role) };
                let s = ItemMark::side(size);
                icon.draw(cx.canvas, Point::new(x, (cy - s / 2.0).round()), s, c);
                s
            }
            ItemMark::Brand => {
                let m = (ItemMark::side(size) / 4.0).floor();
                let y = (cy - 2.0 * m).round();
                let brand = aizome_theme::Brand::for_theme(t.palette.name);
                for ([rx, ry, rw, rh], c) in brand.mark_rects() {
                    let r = Rect::new(
                        x + rx as f64 * m,
                        y + ry as f64 * m,
                        x + (rx + rw) as f64 * m,
                        y + (ry + rh) as f64 * m,
                    );
                    cx.canvas.fill_rect(r, color(c));
                }
                4.0 * m
            }
        }
    }
}
