//! Chanoma's look: a lamp-lit indigo evening (aizome is indigo dyeing),
//! big rounded tiles, Archivo set large enough to read from the sofa, and
//! the amber of Aizome's focus around whatever the controller is on.
//!
//! The controller's buttons are drawn as the controller in hand prints
//! them: Xbox's coloured letters, PlayStation's shapes, Nintendo's
//! letters; and for any other controller, the four buttons as a diamond
//! with the one meant filled in, which reads without letters.

use aizome_kit::layout::{Point, Rect, Size, Vec2};
use aizome_kit::sidestep_ui::kurbo::{BezPath, Circle, Line, RoundedRect};
use aizome_kit::sidestep_ui::{Canvas, Color, LineCap, StrokeStyle};
use aizome_kit::theme::{FontSpec, Theme};

use crate::pad::{Button, Family};

pub const BG_TOP: Color = Color::hex(0x0e1230);
pub const BG_BOTTOM: Color = Color::hex(0x1c2253);
pub const INK: Color = Color::hex(0xf6f1e7);
pub const INK_SOFT: Color = Color::hex(0xb3b8dc);
pub const INK_FAINT: Color = Color::hex(0x7179a9);
pub const CARD: Color = Color::hex(0x272e66);
pub const CARD_EDGE: Color = Color::hex(0x3a4290);
pub const PANEL: Color = Color::hex(0x1f2559);
pub const FOCUS: Color = Color::hex(0xffb43c);
pub const GOOD: Color = Color::hex(0x5cc77c);
pub const WARN: Color = Color::hex(0xffc75a);
pub const DANGER: Color = Color::hex(0xff6b5b);
pub const SHADOW: Color = Color::rgba(0.0, 0.0, 0.05, 0.45);
pub const VEIL: Color = Color::rgba(0.03, 0.04, 0.12, 0.72);

/// Archivo at `size`, `weight`.
pub fn font(size: f64, weight: f32) -> FontSpec {
    FontSpec { weight, ..FontSpec::ui(size) }
}

pub fn measure(theme: &Theme, s: &str, spec: FontSpec) -> Size {
    aizome_kit::sidestep_ui::measure(s, &theme.style(spec, Color::BLACK))
}

/// `s` with its top left at `at`.
pub fn text(c: &mut Canvas, theme: &Theme, s: &str, at: Point, spec: FontSpec, color: Color) {
    c.draw_label(s, &theme.style(spec, color), at);
}

/// `s` centred on `at`.
pub fn text_centred(c: &mut Canvas, theme: &Theme, s: &str, at: Point, spec: FontSpec, color: Color) {
    let size = measure(theme, s, spec);
    text(c, theme, s, Point::new(at.x - size.width / 2.0, at.y - size.height / 2.0), spec, color);
}

/// `s` with its right edge at `x`, its top at `y`.
pub fn text_right(c: &mut Canvas, theme: &Theme, s: &str, x: f64, y: f64, spec: FontSpec, color: Color) {
    let size = measure(theme, s, spec);
    text(c, theme, s, Point::new(x - size.width, y), spec, color);
}

/// `s` cut to `width` with an ellipsis.
pub fn fit_text(theme: &Theme, s: &str, width: f64, spec: FontSpec) -> String {
    if measure(theme, s, spec).width <= width {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if measure(theme, &t, spec).width <= width { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_owned() + "…"
}

/// `s` broken into lines no wider than `width`, at spaces.
pub fn wrap(theme: &Theme, s: &str, width: f64, spec: FontSpec) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if !line.is_empty() && measure(theme, &candidate, spec).width > width {
            lines.push(std::mem::replace(&mut line, word.to_owned()));
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// `s` wrapped to `width`, each line centred, the block's top at `top`:
/// where it ends.
#[allow(clippy::too_many_arguments)]
pub fn paragraph(
    c: &mut Canvas,
    theme: &Theme,
    s: &str,
    centre_x: f64,
    top: f64,
    width: f64,
    spec: FontSpec,
    color: Color,
) -> f64 {
    let line_h = spec.size * 1.32;
    let mut y = top;
    for line in wrap(theme, s, width, spec) {
        text_centred(c, theme, &line, Point::new(centre_x, y + line_h / 2.0), spec, color);
        y += line_h;
    }
    y
}

/// The largest size up to `max` at which `s` fits `width`.
pub fn fitted(theme: &Theme, s: &str, width: f64, max: f64, weight: f32) -> f64 {
    let w = measure(theme, s, font(max, weight)).width;
    if w <= width { max } else { (max * width / w).max(14.0) }
}

/// The sky: flat bands too fine to see (a gradient over the whole window
/// costs milliseconds a frame; flat fills almost nothing).
pub fn backdrop(c: &mut Canvas, b: Rect) {
    let bands = 40;
    for i in 0..bands {
        let t = f64::from(i) / f64::from(bands);
        let y0 = (b.y0 + b.height() * t).floor();
        let y1 = (b.y0 + b.height() * (t + 1.0 / f64::from(bands))).ceil();
        let band = Rect::new(b.x0, y0, b.x1, y1);
        if c.needs(band) {
            c.fill_rect(band, BG_TOP.mix(BG_BOTTOM, (t + 0.5 / f64::from(bands)) as f32));
        }
    }
}

/// A rounded card on Aizome's hard shadow.
pub fn card(c: &mut Canvas, r: Rect, radius: f64, fill: Color) {
    c.fill(&RoundedRect::from_rect(r + Vec2::new(0.0, 8.0), radius), SHADOW);
    c.fill(&RoundedRect::from_rect(r, radius), fill);
}

/// The ring around what's focused.
pub fn focus_ring(c: &mut Canvas, r: Rect, radius: f64) {
    c.stroke(&RoundedRect::from_rect(r.inflate(9.0, 9.0), radius + 9.0), &StrokeStyle::new(6.0), FOCUS);
}

/// A tile's rectangle grown a little while focused.
pub fn lifted(r: Rect, focused: bool) -> Rect {
    if focused { r.inflate(r.width() * 0.04, r.height() * 0.04) } else { r }
}

/// How far drawing reaches outside a tile (ring, lift, shadow): what a
/// part's rectangle must cover.
pub fn reach(r: Rect) -> Rect {
    r.inflate(r.width() * 0.04 + 20.0, r.height() * 0.04 + 22.0)
}

/// A rounded panel for menus and dialogs.
pub fn panel(c: &mut Canvas, r: Rect) {
    card(c, r, 28.0, PANEL);
    c.stroke(&RoundedRect::from_rect(r, 28.0), &StrokeStyle::new(2.0), CARD_EDGE);
}

/// A pill of text (time left, grown-up mode): its rectangle.
pub fn pill(c: &mut Canvas, theme: &Theme, s: &str, right: f64, top: f64, fill: Color, ink: Color) -> Rect {
    let spec = font(28.0, 700.0);
    let size = measure(theme, s, spec);
    let r = Rect::new(right - size.width - 44.0, top, right, top + 52.0);
    c.fill(&RoundedRect::from_rect(r, 26.0), fill);
    text(c, theme, s, Point::new(r.x0 + 22.0, r.y0 + (52.0 - size.height) / 2.0), spec, ink);
    r
}

/// An hourglass, for time.
pub fn hourglass(c: &mut Canvas, at: Point, h: f64, color: Color) {
    let w = h * 0.62;
    let mut p = BezPath::new();
    p.move_to((at.x - w / 2.0, at.y - h / 2.0));
    p.line_to((at.x + w / 2.0, at.y - h / 2.0));
    p.line_to((at.x, at.y));
    p.line_to((at.x + w / 2.0, at.y + h / 2.0));
    p.line_to((at.x - w / 2.0, at.y + h / 2.0));
    p.line_to((at.x, at.y));
    p.close_path();
    c.stroke(&p, &StrokeStyle::new(h * 0.09).with_cap(LineCap::Round), color);
    let mut sand = BezPath::new();
    sand.move_to((at.x - w * 0.3, at.y + h * 0.42));
    sand.line_to((at.x + w * 0.3, at.y + h * 0.42));
    sand.line_to((at.x, at.y + h * 0.12));
    sand.close_path();
    c.fill(&sand, color);
}

/// A crescent moon.
pub fn moon(c: &mut Canvas, at: Point, r: f64, color: Color, sky: Color) {
    c.fill(&Circle::new(at, r), color);
    c.fill(&Circle::new(at + Vec2::new(r * 0.42, -r * 0.28), r * 0.86), sky);
}

/// A padlock, shut.
pub fn padlock(c: &mut Canvas, at: Point, h: f64, color: Color) {
    let w = h * 0.8;
    let body = Rect::new(at.x - w / 2.0, at.y - h * 0.1, at.x + w / 2.0, at.y + h / 2.0);
    c.fill(&RoundedRect::from_rect(body, h * 0.1), color);
    let mut shackle = BezPath::new();
    let sw = w * 0.32;
    shackle.move_to((at.x - sw, at.y - h * 0.1));
    shackle.line_to((at.x - sw, at.y - h * 0.24));
    shackle.curve_to((at.x - sw, at.y - h * 0.52), (at.x + sw, at.y - h * 0.52), (at.x + sw, at.y - h * 0.24));
    shackle.line_to((at.x + sw, at.y - h * 0.1));
    c.stroke(&shackle, &StrokeStyle::new(h * 0.11).with_cap(LineCap::Round), color);
}

/// A game controller's outline, `w` wide.
pub fn controller(c: &mut Canvas, at: Point, w: f64, color: Color, fill: Color) {
    let h = w * 0.62;
    let mut p = BezPath::new();
    let (x0, y0) = (at.x - w / 2.0, at.y - h / 2.0);
    p.move_to((x0 + w * 0.22, y0 + h * 0.08));
    p.line_to((x0 + w * 0.78, y0 + h * 0.08));
    p.curve_to((x0 + w * 0.95, y0 + h * 0.08), (x0 + w * 1.02, y0 + h * 0.75), (x0 + w * 0.95, y0 + h * 0.95));
    p.curve_to((x0 + w * 0.88, y0 + h * 1.08), (x0 + w * 0.74, y0 + h * 0.98), (x0 + w * 0.66, y0 + h * 0.74));
    p.line_to((x0 + w * 0.34, y0 + h * 0.74));
    p.curve_to((x0 + w * 0.26, y0 + h * 0.98), (x0 + w * 0.12, y0 + h * 1.08), (x0 + w * 0.05, y0 + h * 0.95));
    p.curve_to((x0 - w * 0.02, y0 + h * 0.75), (x0 + w * 0.05, y0 + h * 0.08), (x0 + w * 0.22, y0 + h * 0.08));
    p.close_path();
    c.fill(&p, fill);
    c.stroke(&p, &StrokeStyle::new(w * 0.035), color);
    // The arrows and the four buttons.
    let cross = StrokeStyle::new(w * 0.045).with_cap(LineCap::Round);
    let dp = Point::new(x0 + w * 0.27, y0 + h * 0.4);
    let arm = w * 0.07;
    c.stroke(&Line::new((dp.x - arm, dp.y), (dp.x + arm, dp.y)), &cross, color);
    c.stroke(&Line::new((dp.x, dp.y - arm), (dp.x, dp.y + arm)), &cross, color);
    let bp = Point::new(x0 + w * 0.73, y0 + h * 0.4);
    for (dx, dy) in [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
        c.fill(&Circle::new(bp + Vec2::new(dx * arm, dy * arm), w * 0.03), color);
    }
    // The button in the middle that wakes it.
    c.fill(&Circle::new(Point::new(at.x, y0 + h * 0.26), w * 0.04), FOCUS);
}

/// How a family prints a face button: its letter (or none for a shape),
/// its colour and its letters' colour.
fn face(family: Family, b: Button) -> (Option<&'static str>, Color, Color) {
    let dark = Color::hex(0x2b2f45);
    match family {
        Family::Xbox => match b {
            Button::South => (Some("A"), Color::hex(0x3fae49), Color::WHITE),
            Button::East => (Some("B"), Color::hex(0xe0383e), Color::WHITE),
            Button::West => (Some("X"), Color::hex(0x2a7bd6), Color::WHITE),
            _ => (Some("Y"), Color::hex(0xf2b31a), Color::hex(0x1b1b1b)),
        },
        Family::Nintendo => match b {
            Button::South => (Some("B"), dark, Color::WHITE),
            Button::East => (Some("A"), dark, Color::WHITE),
            Button::West => (Some("Y"), dark, Color::WHITE),
            _ => (Some("X"), dark, Color::WHITE),
        },
        Family::PlayStation | Family::Generic | Family::Keyboard => (None, dark, Color::WHITE),
    }
}

/// The key on a keyboard that does what button `b` does here.
fn key_for(b: Button) -> &'static str {
    match b {
        Button::South => "Return",
        Button::East => "Esc",
        Button::West => "Space",
        Button::North => "Tab",
        Button::Guide => "Home",
        Button::Start => "Menu",
        Button::L1 => "Page Up",
        Button::R1 => "Page Down",
        _ => "",
    }
}

/// How wide button `b`'s glyph is at radius `r`.
pub fn glyph_width(theme: &Theme, family: Family, b: Button, r: f64) -> f64 {
    if family == Family::Keyboard {
        return measure(theme, key_for(b), font(r * 0.8, 700.0)).width + r * 1.1;
    }
    match b {
        Button::South | Button::East | Button::West | Button::North | Button::Guide => r * 2.0,
        Button::Start | Button::Select if family != Family::Nintendo => r * 2.0,
        _ => r * 2.4,
    }
}

/// Button `b` as the controller prints it, `r` its radius.
pub fn button_glyph(c: &mut Canvas, theme: &Theme, family: Family, b: Button, at: Point, r: f64) {
    if family == Family::Keyboard {
        let spec = font(r * 0.8, 700.0);
        let w = glyph_width(theme, family, b, r);
        let cap = Rect::new(at.x - w / 2.0, at.y - r * 0.8, at.x + w / 2.0, at.y + r * 0.8);
        c.fill(&RoundedRect::from_rect(cap + Vec2::new(0.0, 3.0), r * 0.3), Color::hex(0x10142e));
        c.fill(&RoundedRect::from_rect(cap, r * 0.3), Color::hex(0x2f3670));
        text_centred(c, theme, key_for(b), at, spec, INK);
        return;
    }
    match b {
        Button::South | Button::East | Button::West | Button::North => {
            let (letter, fill, ink) = face(family, b);
            c.fill(&Circle::new(at, r), fill);
            if let Some(l) = letter {
                text_centred(c, theme, l, at + Vec2::new(0.0, r * 0.04), font(r * 1.15, 800.0), ink);
            } else if family == Family::PlayStation {
                let s = StrokeStyle::new(r * 0.16).with_cap(LineCap::Round);
                let k = r * 0.45;
                match b {
                    Button::South => {
                        let col = Color::hex(0x8fb4ea);
                        c.stroke(&Line::new((at.x - k, at.y - k), (at.x + k, at.y + k)), &s, col);
                        c.stroke(&Line::new((at.x + k, at.y - k), (at.x - k, at.y + k)), &s, col);
                    }
                    Button::East => c.stroke(&Circle::new(at, k * 1.05), &s, Color::hex(0xee6f7a)),
                    Button::West => c.stroke(
                        &Rect::new(at.x - k * 0.9, at.y - k * 0.9, at.x + k * 0.9, at.y + k * 0.9),
                        &s,
                        Color::hex(0xe39bd0),
                    ),
                    _ => {
                        let mut t = BezPath::new();
                        t.move_to((at.x, at.y - k * 1.05));
                        t.line_to((at.x + k * 1.05, at.y + k * 0.75));
                        t.line_to((at.x - k * 1.05, at.y + k * 0.75));
                        t.close_path();
                        c.stroke(&t, &s, Color::hex(0x5fd0b3));
                    }
                }
            } else {
                // The diamond: the button meant filled, the others hollow.
                let k = r * 0.42;
                let dot = r * 0.2;
                for (bb, dx, dy) in [
                    (Button::North, 0.0, -1.0),
                    (Button::East, 1.0, 0.0),
                    (Button::South, 0.0, 1.0),
                    (Button::West, -1.0, 0.0),
                ] {
                    let p = at + Vec2::new(dx * k, dy * k);
                    if bb == b {
                        c.fill(&Circle::new(p, dot * 1.25), FOCUS);
                    } else {
                        c.stroke(&Circle::new(p, dot), &StrokeStyle::new(r * 0.07), INK_SOFT);
                    }
                }
            }
        }
        Button::Start | Button::Select | Button::Guide if family != Family::Nintendo || b == Button::Guide => {
            // Drawn, not lettered: Xbox's menu lines and view squares, a
            // house for Home.
            c.fill(&Circle::new(at, r), Color::hex(0x2b2f45));
            let s = StrokeStyle::new(r * 0.13).with_cap(LineCap::Round);
            let k = r * 0.42;
            match b {
                Button::Start => {
                    for dy in [-k * 0.7, 0.0, k * 0.7] {
                        c.stroke(&Line::new((at.x - k, at.y + dy), (at.x + k, at.y + dy)), &s, INK);
                    }
                }
                Button::Select => {
                    let a = Rect::new(at.x - k, at.y - k * 0.75, at.x + k * 0.35, at.y + k * 0.45);
                    c.stroke(&RoundedRect::from_rect(a, r * 0.08), &s, INK);
                    let b2 = a + Vec2::new(k * 0.65, k * 0.35);
                    c.fill(&RoundedRect::from_rect(b2, r * 0.08), Color::hex(0x2b2f45));
                    c.stroke(&RoundedRect::from_rect(b2, r * 0.08), &s, INK);
                }
                _ => {
                    let mut house = BezPath::new();
                    house.move_to((at.x - k, at.y - k * 0.05));
                    house.line_to((at.x, at.y - k));
                    house.line_to((at.x + k, at.y - k * 0.05));
                    house.move_to((at.x - k * 0.7, at.y - k * 0.3));
                    house.line_to((at.x - k * 0.7, at.y + k * 0.85));
                    house.line_to((at.x + k * 0.7, at.y + k * 0.85));
                    house.line_to((at.x + k * 0.7, at.y - k * 0.3));
                    c.stroke(&house, &s, INK);
                }
            }
        }
        _ => {
            let label = match b {
                Button::Start => "+",
                Button::Select => "−",
                Button::L1 => "L",
                Button::R1 => "R",
                Button::L2 => "L2",
                Button::R2 => "R2",
                _ => "•",
            };
            let label = match (family, b) {
                (Family::PlayStation, Button::L1) => "L1",
                (Family::PlayStation, Button::R1) => "R1",
                (Family::Xbox, Button::L1) => "LB",
                (Family::Xbox, Button::R1) => "RB",
                (Family::Xbox, Button::L2) => "LT",
                (Family::Xbox, Button::R2) => "RT",
                (Family::Nintendo, Button::L2) => "ZL",
                (Family::Nintendo, Button::R2) => "ZR",
                _ => label,
            };
            let w = (measure(theme, label, font(r * 0.95, 800.0)).width + r * 0.9).max(r * 2.0);
            let rr = Rect::new(at.x - w / 2.0, at.y - r * 0.8, at.x + w / 2.0, at.y + r * 0.8);
            c.fill(&RoundedRect::from_rect(rr, r * 0.8), Color::hex(0x2b2f45));
            text_centred(c, theme, label, at, font(r * 0.95, 800.0), INK);
        }
    }
}

/// A hint at the screen's foot: a button and what it does. Returns where
/// the next starts.
pub fn hint(c: &mut Canvas, theme: &Theme, family: Family, b: Button, what: &str, x: f64, y: f64) -> f64 {
    let r = 22.0;
    let w = glyph_width(theme, family, b, r);
    button_glyph(c, theme, family, b, Point::new(x + w / 2.0, y), r);
    let spec = font(28.0, 600.0);
    let size = measure(theme, what, spec);
    text(c, theme, what, Point::new(x + w + 14.0, y - size.height / 2.0), spec, INK_SOFT);
    x + w + 14.0 + size.width + 44.0
}

/// A battery, `fill` from 0 to 1.
pub fn battery(c: &mut Canvas, at: Point, w: f64, fill: f64, charging: bool) {
    let h = w * 0.5;
    let body = Rect::new(at.x - w / 2.0, at.y - h / 2.0, at.x + w / 2.0 - w * 0.08, at.y + h / 2.0);
    c.stroke(&RoundedRect::from_rect(body, h * 0.2), &StrokeStyle::new(w * 0.07), INK_SOFT);
    c.fill_rect(Rect::new(body.x1, at.y - h * 0.2, body.x1 + w * 0.08, at.y + h * 0.2), INK_SOFT);
    let inner = body.inflate(-w * 0.09, -w * 0.09);
    let color = if charging {
        GOOD
    } else if fill < 0.2 {
        DANGER
    } else {
        INK
    };
    c.fill_rect(Rect::new(inner.x0, inner.y0, inner.x0 + inner.width() * fill.clamp(0.05, 1.0), inner.y1), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_are_where_each_maker_prints_them() {
        assert_eq!(face(Family::Xbox, Button::South).0, Some("A"));
        assert_eq!(face(Family::Nintendo, Button::East).0, Some("A"));
        assert_eq!(face(Family::Nintendo, Button::South).0, Some("B"));
        assert_eq!(face(Family::PlayStation, Button::South).0, None);
    }
}
