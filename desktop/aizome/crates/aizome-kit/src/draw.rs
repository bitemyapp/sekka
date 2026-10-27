//! Aizome's drawing vocabulary: square edges on whole points, 1pt bevels
//! (`bevel_hi` along the top inside, `bevel_lo` around the outside), the
//! hard 3pt shadow under floating things, and the 2pt focus ring.
//!
//! Edges are filled rectangles rather than strokes, so they land on whole
//! device pixels at every integer scale with no smoothing.

use sidestep_ui::{Canvas, Color};

use crate::layout::Rect;
use crate::theme::Theme;

/// A 1pt (or `w`) border inside `r`.
pub fn border(canvas: &mut Canvas, r: Rect, color: Color, w: f64) {
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    canvas.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + w), color);
    canvas.fill_rect(Rect::new(r.x0, r.y1 - w, r.x1, r.y1), color);
    canvas.fill_rect(Rect::new(r.x0, r.y0 + w, r.x0 + w, r.y1 - w), color);
    canvas.fill_rect(Rect::new(r.x1 - w, r.y0 + w, r.x1, r.y1 - w), color);
}

/// A line along the top of `r`, `w` thick.
pub fn top(canvas: &mut Canvas, r: Rect, color: Color, w: f64) {
    canvas.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + w), color);
}

pub fn bottom(canvas: &mut Canvas, r: Rect, color: Color, w: f64) {
    canvas.fill_rect(Rect::new(r.x0, r.y1 - w, r.x1, r.y1), color);
}

pub fn left(canvas: &mut Canvas, r: Rect, color: Color, w: f64) {
    canvas.fill_rect(Rect::new(r.x0, r.y0, r.x0 + w, r.y1), color);
}

pub fn right(canvas: &mut Canvas, r: Rect, color: Color, w: f64) {
    canvas.fill_rect(Rect::new(r.x1 - w, r.y0, r.x1, r.y1), color);
}

/// A raised face: `fill`, a 1pt `edge` border, and the top highlight just
/// inside it. The board's toolbar buttons and pop-ups.
pub fn raised(canvas: &mut Canvas, theme: &Theme, r: Rect, fill: Option<Color>, edge: Color) {
    if let Some(f) = fill {
        canvas.fill_rect(r, f);
    }
    border(canvas, r, edge, 1.0);
    let inner = r.inset(-1.0);
    top(canvas, inner, theme.bevel_hi(), 1.0);
}

/// A sunken well: `fill`, a 1pt `edge` border, and a shade along the top
/// inside, as fields and tracks are.
pub fn sunken(canvas: &mut Canvas, theme: &Theme, r: Rect, fill: Color, edge: Color) {
    canvas.fill_rect(r, fill);
    border(canvas, r, edge, 1.0);
    let inner = r.inset(-1.0);
    let shade = if theme.is_dark() { theme.bevel_lo().with_alpha(0.55) } else { theme.bevel_lo().with_alpha(0.18) };
    top(canvas, inner, shade, 1.0);
}

/// A floating panel's look (menus, popovers, sheets): `raised`, a 1pt
/// `bevel_lo` edge, the top highlight, and the hard shadow 3pt right and
/// down when effects allow.
pub fn floating(canvas: &mut Canvas, theme: &Theme, r: Rect) {
    if theme.shadows() {
        let s = theme.metrics.shadow;
        canvas.fill_rect(Rect::new(r.x0 + s, r.y1, r.x1 + s, r.y1 + s), theme.slab());
        canvas.fill_rect(Rect::new(r.x1, r.y0 + s, r.x1 + s, r.y1), theme.slab());
    }
    canvas.fill_rect(r, theme.raised());
    border(canvas, r, theme.bevel_lo(), 1.0);
    top(canvas, r.inset(-1.0), theme.bevel_hi(), 1.0);
}

/// A menu's look: [`floating`]'s, frosted where the effects are Full.
pub fn menu_face(canvas: &mut Canvas, theme: &Theme, r: Rect) {
    if !theme.frosted() {
        floating(canvas, theme, r);
        return;
    }
    if theme.shadows() {
        let s = theme.metrics.shadow;
        canvas.fill_rect(Rect::new(r.x0 + s, r.y1, r.x1 + s, r.y1 + s), theme.slab());
        canvas.fill_rect(Rect::new(r.x1, r.y0 + s, r.x1 + s, r.y1), theme.slab());
    }
    frost(canvas, theme, r, theme.raised());
    border(canvas, r, theme.bevel_lo(), 1.0);
    top(canvas, r.inset(-1.0), theme.bevel_hi(), 1.0);
}

/// How opaque a frosted face is in Night: what's under it tints through.
/// There's no blur, so what shows through shows sharp: this keeps text
/// under a menu (a desktop label, a window's) to a faint ghost, and the
/// faintest text on the face (`text3`) at 3:1 or better over a white
/// light.
pub const FROST_ALPHA: f32 = 0.90;

/// How opaque a frosted face is in Day: lighter faces show the darker
/// things under them more, so more of the face.
pub const FROST_ALPHA_DAY: f32 = 0.94;

/// `r` filled with `face`: where the effects are Full, frosted (the colour
/// at [`FROST_ALPHA`] (Night) or [`FROST_ALPHA_DAY`] with a fine static
/// grain over it, no blur), else solid. The grain is one small tile made
/// once, drawn a pixel to a device pixel, so it's as fine as the screen.
pub fn frost(canvas: &mut Canvas, theme: &Theme, r: Rect, face: Color) {
    if !theme.frosted() {
        canvas.fill_rect(r, face);
        return;
    }
    let alpha = if theme.is_dark() { FROST_ALPHA } else { FROST_ALPHA_DAY };
    canvas.fill_rect(r, face.with_alpha(alpha));
    let tile = grain();
    // A tile pixel to a device pixel: at 2× the tile is 32 points.
    let n = GRAIN as f64 / canvas.scale().max(1.0);
    let opts = sidestep_ui::ImageOptions { smooth: false, ..Default::default() };
    canvas.save();
    canvas.clip_rect(r);
    let mut y = r.y0;
    while y < r.y1 {
        let mut x = r.x0;
        while x < r.x1 {
            canvas.draw_image_with(tile, None, Rect::new(x, y, x + n, y + n), opts);
            x += n;
        }
        y += n;
    }
    canvas.restore();
}

/// The grain tile's side, in pixels.
const GRAIN: u32 = 64;

/// Fine noise at about 4% contrast: specks a little lighter or darker,
/// the same everywhere and every time.
fn grain() -> &'static sidestep_ui::Image {
    static TILE: std::sync::OnceLock<sidestep_ui::Image> = std::sync::OnceLock::new();
    TILE.get_or_init(|| {
        let mut state: u32 = 0x9e37_79b9;
        let mut px = Vec::with_capacity((GRAIN * GRAIN * 4) as usize);
        for _ in 0..GRAIN * GRAIN {
            // xorshift32
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let v = (state >> 24) as i32 - 128;
            let a = (v.unsigned_abs() * 20 / 128) as u8;
            let c = if v >= 0 { 255 } else { 0 };
            px.extend([c, c, c, a]);
        }
        sidestep_ui::Image::from_rgba(GRAIN, GRAIN, &px).expect("the grain's pixels")
    })
}

/// The 2pt focus ring outside `r`.
pub fn focus_ring(canvas: &mut Canvas, r: Rect, color: Color) {
    border(canvas, r.inflate(2.0, 2.0), color, 2.0);
}

/// A separator: a `line` rule with the highlight under it.
pub fn separator(canvas: &mut Canvas, theme: &Theme, x0: f64, x1: f64, y: f64) {
    canvas.fill_rect(Rect::new(x0, y, x1, y + 1.0), theme.line());
    canvas.fill_rect(Rect::new(x0, y + 1.0, x1, y + 2.0), theme.bevel_hi());
}

/// A rectangle with `r`'s centre, `w` by `h`, on whole points.
pub fn centered(r: Rect, w: f64, h: f64) -> Rect {
    let x = (r.x0 + (r.width() - w) / 2.0).round();
    let y = (r.y0 + (r.height() - h) / 2.0).round();
    Rect::new(x, y, x + w, y + h)
}
