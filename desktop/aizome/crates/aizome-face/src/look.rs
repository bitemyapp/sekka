//! Aizome's drawing vocabulary over the kit's, shared by the shell and the
//! login screen: the mark, hard shadows,
//! menu title strips, keycaps, and the text specs the boards use
//! (`aizome-design/reference/html/C2-*.html`).

use aizome_kit::draw;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::{Canvas, Color};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, Theme, color};
use aizome_theme::{Brand, typeface};

/// A colour of the palette, by name, for things that pick one ahead of
/// the theme (an app's glyph).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Hue {
    #[default]
    Text,
    Text2,
    Text3,
    Blue,
    Cyan,
    Green,
    Red,
    Violet,
    Amber,
    Focus,
}

impl Hue {
    pub fn color(self, t: &Theme) -> Color {
        let p = &t.palette;
        color(match self {
            Hue::Text => p.text,
            Hue::Text2 => p.text2,
            Hue::Text3 => p.text3,
            Hue::Blue => p.blue,
            Hue::Cyan => p.cyan,
            Hue::Green => p.green,
            Hue::Red => p.red,
            Hue::Violet => p.violet,
            Hue::Amber => p.amber,
            Hue::Focus => p.focus,
        })
    }
}

/// Archivo at `size` and `weight`.
pub fn ui(size: f64, weight: f32, c: Color) -> TextSpec {
    let mut f = FontSpec::ui(size);
    f.weight = weight;
    TextSpec::new(f, c)
}

/// Archivo at 87.5% width, as tabs, the Toolchest button and menu titles.
pub fn condensed(size: f64, weight: f32, c: Color) -> TextSpec {
    let mut s = ui(size, weight, c);
    s.font.condensed = true;
    s
}

/// IBM Plex Mono (with the modifier glyphs from Noto Sans Symbols 2).
pub fn mono(size: f64, weight: f32, c: Color) -> TextSpec {
    let mut f = FontSpec::mono(size);
    f.weight = weight;
    TextSpec::new(f, c)
}

/// Mono with tabular digits: the clock, percentages.
pub fn digits(size: f64, weight: f32, c: Color) -> TextSpec {
    let mut s = mono(size, weight, c);
    s.font.tabular = true;
    s
}

/// The mono uppercase section heads ("SHELF", "COMMANDS").
pub fn caption(t: &Theme) -> TextSpec {
    let mut s = mono(10.5, typeface::WEIGHT_MEDIUM, t.text3());
    s.letter_spacing = 10.5 * 0.12;
    s
}

/// Aizome's mark `size` points square at `at`: the amber tab and three
/// indigo stripes, on whole device pixels.
pub fn mark(canvas: &mut Canvas, t: &Theme, at: Point, size: f64) {
    let brand = Brand::for_theme(t.palette.name);
    let m = size / 4.0;
    for ([x, y, w, h], c) in brand.mark_rects() {
        let r =
            Rect::new(at.x + x as f64 * m, at.y + y as f64 * m, at.x + (x + w) as f64 * m, at.y + (y + h) as f64 * m);
        canvas.fill_rect(r, color(c));
    }
}

/// The hard shadow of a floating thing at `r`: 3pt right and down, never
/// blurred (nothing in Austere).
pub fn shadow(canvas: &mut Canvas, t: &Theme, r: Rect) {
    if t.shadows() {
        let s = t.metrics.shadow;
        canvas.fill_rect(Rect::new(r.x0 + s, r.y1, r.x1 + s, r.y1 + s), t.slab());
        canvas.fill_rect(Rect::new(r.x1, r.y0 + s, r.x1 + s, r.y1), t.slab());
    }
}

/// The Deskbar's shadow: 3pt left and down (it hangs from the right edge).
pub fn shadow_left(canvas: &mut Canvas, t: &Theme, r: Rect) {
    if t.shadows() {
        let s = t.metrics.shadow;
        canvas.fill_rect(Rect::new(r.x0 - s, r.y1, r.x1 - s, r.y1 + s), t.slab());
        canvas.fill_rect(Rect::new(r.x0 - s, r.y0 + s, r.x0, r.y1), t.slab());
    }
}

/// A floating panel's face: `raised` (frosted in Full), the 1pt `bevel_lo` edge and the top
/// highlight, with its shadow.
pub fn panel(canvas: &mut Canvas, t: &Theme, r: Rect) {
    shadow(canvas, t, r);
    // Frosted where the effects are Full (translucent, a fine grain).
    draw::frost(canvas, t, r, t.raised());
    draw::border(canvas, r, t.bevel_lo(), 1.0);
    draw::top(canvas, r.inset(-1.0), t.bevel_hi(), 1.0);
}

/// A square box drawn as the tabs' close box: `size` square, 1.5pt edges.
pub fn close_box(canvas: &mut Canvas, r: Rect, c: Color) {
    draw::border(canvas, r, c, 1.5);
}

/// The pin of a menu's title strip (tear off / keep): the same glyph
/// either way, in `focus` when kept (the caller's colour).
pub fn pin(canvas: &mut Canvas, at: Point, size: f64, c: Color) {
    aizome_kit::Icon::Pin.draw(canvas, at, size, c);
}

/// A keycap: the label in mono on `surface`, edged in `line2` with a 3pt
/// bottom edge (Strip's `.key`).
pub fn keycap(canvas: &mut Canvas, t: &Theme, r: Rect) {
    canvas.fill_rect(r, t.surface());
    draw::border(canvas, r, t.line2(), 1.0);
    draw::bottom(canvas, r, t.line2(), 3.0);
}

/// A colour between two, `k` of the way to `b`.
pub fn mix(a: Color, b: Color, k: f32) -> Color {
    a.mix(b, k)
}

/// The focus colour at `alpha` over nothing: the Deskbar's focused row.
pub fn focus_wash(t: &Theme, alpha: f32) -> Color {
    t.focus().with_alpha(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hues_follow_the_theme() {
        let night = Theme::night();
        let day = Theme::day();
        assert_ne!(Hue::Blue.color(&night), Hue::Blue.color(&day));
        assert_eq!(Hue::Focus.color(&night), night.focus());
    }
}

/// A Lucide glyph stroked `stroke` points wide (the desktop's large icons
/// draw thinner than the kit's proportional weight).
pub fn glyph(canvas: &mut Canvas, icon: aizome_kit::Icon, at: Point, size: f64, c: Color, stroke: f64) {
    use aizome_kit::sidestep_ui::kurbo::Affine;
    use aizome_kit::sidestep_ui::{LineCap, LineJoin, StrokeStyle};
    let mut path = (*icon.path()).clone();
    path.apply_affine(Affine::translate(at.to_vec2()) * Affine::scale(size / 24.0));
    canvas.stroke(&path, &StrokeStyle::new(stroke).with_cap(LineCap::Round).with_join(LineJoin::Round), c);
}

/// An image in a model that compares (by its size: enough to see a change
/// of icon).
#[derive(Clone, Debug)]
pub struct Pic(pub aizome_kit::Image);

impl PartialEq for Pic {
    fn eq(&self, other: &Pic) -> bool {
        self.0.pixel_size() == other.0.pixel_size()
    }
}
