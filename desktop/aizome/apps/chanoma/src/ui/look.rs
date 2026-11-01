//! Chanoma's look: Aizome's, seen from the sofa. A television across a
//! room reads about as a laptop's screen at arm's length does at twice
//! the size, so Aizome's points are doubled here ([`K`]): its 1pt edges
//! and bevels are 2, the 2pt focus ring 4, the 3pt hard shadow 6.
//! Everything else is as the desktop draws it: the theme's colours (Night,
//! Day or High contrast, and the focus colour chosen in Settings), square
//! corners, flat faces with a highlight along the top, Archivo for words,
//! IBM Plex Mono for clocks, numbers and spaced capitals over sections,
//! and the kit's Lucide icons.
//!
//! The controller's buttons are drawn as the controller in hand prints
//! them: Xbox's letters in their colours, PlayStation's shapes, Nintendo's
//! letters; for any other controller, the four buttons as a diamond with
//! the one meant filled in, which reads without letters. Their colours are
//! the theme's green, red, blue and amber, so they hold their contrast in
//! every theme.

use aizome_kit::Icon;
use aizome_kit::draw::{border, top};
use aizome_kit::layout::{Point, Rect, Size, Vec2};
use aizome_kit::sidestep_ui::kurbo::{BezPath, Circle};
use aizome_kit::sidestep_ui::{Canvas, Color, LineCap, StrokeStyle, TextStyle};
use aizome_kit::theme::{FontSpec, Role, Theme};

use crate::pad::{Button, Family};

/// Aizome's points in the design's: everything twice its desktop size.
pub const K: f64 = 2.0;
/// An edge, a bevel, a rule.
pub const EDGE: f64 = K;
/// The focus ring.
pub const RING: f64 = 2.0 * K;
/// The hard shadow's offset, right and down.
pub const SLAB: f64 = 3.0 * K;
/// The rail beside what's current (a menu's lit row, a note).
pub const RAIL: f64 = 3.0 * K;

/// Type sizes: Aizome's, doubled and rounded up a little for the room.
pub const CAPTION: f64 = 20.0;
pub const SMALL: f64 = 24.0;
pub const BODY: f64 = 28.0;
pub const TITLE: f64 = 32.0;
pub const HEADLINE: f64 = 44.0;
pub const DISPLAY: f64 = 60.0;

pub const REGULAR: f32 = 400.0;
pub const MEDIUM: f32 = 500.0;
pub const SEMIBOLD: f32 = 600.0;
pub const BOLD: f32 = 750.0;

/// Archivo at `size`, `weight`.
pub fn font(size: f64, weight: f32) -> FontSpec {
    FontSpec { weight, ..FontSpec::ui(size) }
}

/// IBM Plex Mono at `size`, `weight`, with digits all one width.
pub fn mono(size: f64, weight: f32) -> FontSpec {
    FontSpec { weight, tabular: true, ..FontSpec::mono(size) }
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

/// `s` with its left edge at `x`, centred on `y`: its width.
pub fn text_mid(c: &mut Canvas, theme: &Theme, s: &str, x: f64, y: f64, spec: FontSpec, color: Color) -> f64 {
    let size = measure(theme, s, spec);
    text(c, theme, s, Point::new(x, y - size.height / 2.0), spec, color);
    size.width
}

/// `s` with its right edge at `x`, centred on `y`: its width.
pub fn text_right(c: &mut Canvas, theme: &Theme, s: &str, x: f64, y: f64, spec: FontSpec, color: Color) -> f64 {
    let size = measure(theme, s, spec);
    text(c, theme, s, Point::new(x - size.width, y - size.height / 2.0), spec, color);
    size.width
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
    let line_h = spec.size * 1.4;
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

/// A caption's style: mono capitals, spaced, as Aizome heads its
/// sections and menus' groups.
fn caption_style(theme: &Theme, color: Color, size: f64) -> TextStyle {
    let mut style = theme.style(mono(size, MEDIUM), color);
    style.letter_spacing = 1.2 * size / 10.5;
    style
}

/// A caption (`s` set in capitals) with its left edge at `x`, centred on
/// `y`, in `color`: its width.
pub fn caption(c: &mut Canvas, theme: &Theme, s: &str, x: f64, y: f64, color: Color) -> f64 {
    caption_sized(c, theme, s, x, y, CAPTION, color)
}

pub fn caption_sized(c: &mut Canvas, theme: &Theme, s: &str, x: f64, y: f64, size: f64, color: Color) -> f64 {
    let s = s.to_uppercase();
    let style = caption_style(theme, color, size);
    let m = aizome_kit::sidestep_ui::measure(&s, &style);
    c.draw_label(&s, &style, Point::new(x, y - m.height / 2.0));
    m.width
}

pub fn caption_width(theme: &Theme, s: &str, size: f64) -> f64 {
    aizome_kit::sidestep_ui::measure(&s.to_uppercase(), &caption_style(theme, Color::BLACK, size)).width
}

/// Chanoma's ground: the theme's surface, flat.
pub fn backdrop(c: &mut Canvas, theme: &Theme, b: Rect) {
    c.fill_rect(b, theme.surface());
}

/// A face (a tile, a row, a button): `fill`, an `edge` around it and the
/// highlight along the top inside, as the kit's raised faces are.
pub fn face(c: &mut Canvas, theme: &Theme, r: Rect, fill: Color, edge: Color) {
    c.fill_rect(r, fill);
    border(c, r, edge, EDGE);
    top(c, r.inset(-EDGE), theme.bevel_hi(), EDGE);
}

/// A panel that floats (a menu, the PIN pad, a note): `raised`, a
/// `bevel_lo` edge, the highlight, and the hard shadow right and down
/// where the effects allow.
pub fn floating(c: &mut Canvas, theme: &Theme, r: Rect) {
    if theme.shadows() {
        c.fill_rect(Rect::new(r.x0 + SLAB, r.y1, r.x1 + SLAB, r.y1 + SLAB), theme.slab());
        c.fill_rect(Rect::new(r.x1, r.y0 + SLAB, r.x1 + SLAB, r.y1), theme.slab());
    }
    face(c, theme, r, theme.raised(), theme.bevel_lo());
}

/// A well (a field, a track, a picture's mat): `sunk`, a `line2` edge and
/// a shade along the top inside.
pub fn well(c: &mut Canvas, theme: &Theme, r: Rect) {
    c.fill_rect(r, theme.sunk());
    border(c, r, theme.line2(), EDGE);
    let shade = if theme.is_dark() { theme.bevel_lo().with_alpha(0.55) } else { theme.bevel_lo().with_alpha(0.18) };
    top(c, r.inset(-EDGE), shade, EDGE);
}

/// Lit: the selection's indigo, as the kit's primary buttons and menus'
/// rows under the pointer are. Text on it is `on_selection`.
pub fn lit(c: &mut Canvas, theme: &Theme, r: Rect) {
    c.fill_rect(r, theme.selection());
    border(c, r, theme.bevel_lo(), EDGE);
    top(c, r.inset(-EDGE), Color::WHITE.with_alpha(0.16), EDGE);
}

/// The ring around what the controller is on, outside `r`.
pub fn ring(c: &mut Canvas, theme: &Theme, r: Rect) {
    border(c, r.inflate(RING, RING), theme.focus(), RING);
}

/// How far drawing reaches outside a tile (its ring): what a part's
/// rectangle must cover.
pub fn reach(r: Rect) -> Rect {
    r.inflate(RING + 2.0, RING + 2.0)
}

/// A rule across, with the highlight under it.
pub fn rule(c: &mut Canvas, theme: &Theme, x0: f64, x1: f64, y: f64) {
    c.fill_rect(Rect::new(x0, y, x1, y + EDGE), theme.line());
    c.fill_rect(Rect::new(x0, y + EDGE, x1, y + 2.0 * EDGE), theme.bevel_hi());
}

/// A push button with its label centred: lit and ringed while the
/// controller is on it, else a raised face.
pub fn button(c: &mut Canvas, theme: &Theme, r: Rect, label: &str, focused: bool) {
    let ink = if focused {
        lit(c, theme, r);
        ring(c, theme, r);
        theme.on_selection()
    } else {
        face(c, theme, r, theme.raised(), theme.line2());
        theme.text()
    };
    text_centred(c, theme, label, r.center(), font(TITLE - 2.0, SEMIBOLD), ink);
}

/// A chip of capitals (`NEW`, `GROWN-UP`) with its right edge at `right`,
/// centred on `y`, in the focus colour: its rectangle.
pub fn chip(c: &mut Canvas, theme: &Theme, s: &str, right: f64, y: f64, size: f64) -> Rect {
    let w = caption_width(theme, s, size) + size * 1.2;
    let h = size * 1.8;
    let r = Rect::new((right - w).round(), (y - h / 2.0).round(), right.round(), (y + h / 2.0).round());
    c.fill_rect(r, theme.focus());
    caption_sized(c, theme, s, r.x0 + size * 0.6, r.center().y, size, theme.on_focus());
    r
}

/// A Lucide icon `size` square, centred on `at`.
pub fn icon(c: &mut Canvas, i: Icon, at: Point, size: f64, color: Color) {
    i.draw(c, Point::new(at.x - size / 2.0, at.y - size / 2.0), size, color);
}

/// Aizome's mark (a dyer's cloth: the amber tab and three indigo
/// stripes), 4 `m` square, its top left at `at`.
pub fn mark(c: &mut Canvas, theme: &Theme, at: Point, m: f64) {
    let brand = aizome_theme::Brand::for_theme(theme.palette.name);
    for ([x, y, w, h], col) in brand.mark_rects() {
        let r = Rect::new(
            at.x + f64::from(x) * m,
            at.y + f64::from(y) * m,
            at.x + f64::from(x + w) * m,
            at.y + f64::from(y + h) * m,
        );
        c.fill_rect(r, aizome_kit::theme::color(col));
    }
}

/// A battery's icon for `pct` (charging or not), and its colour.
pub fn battery(theme: &Theme, pct: u8, charging: bool) -> (Icon, Color) {
    match (charging, pct) {
        (true, _) => (Icon::BatteryCharging, theme.role(Role::Success)),
        (false, 0..15) => (Icon::BatteryWarning, theme.role(Role::Warning)),
        (false, 15..40) => (Icon::BatteryLow, theme.text2()),
        (false, 40..75) => (Icon::BatteryMedium, theme.text2()),
        (false, _) => (Icon::BatteryFull, theme.text2()),
    }
}

/// How a family prints a face button: its letter (or none for a shape)
/// and the colour it's printed in.
fn face_print(theme: &Theme, family: Family, b: Button) -> (Option<&'static str>, Color) {
    match family {
        Family::Xbox => match b {
            Button::South => (Some("A"), theme.role(Role::Success)),
            Button::East => (Some("B"), theme.role(Role::Danger)),
            Button::West => (Some("X"), theme.role(Role::Accent)),
            _ => (Some("Y"), theme.role(Role::Warning)),
        },
        Family::Nintendo => match b {
            Button::South => (Some("B"), theme.text()),
            Button::East => (Some("A"), theme.text()),
            Button::West => (Some("Y"), theme.text()),
            _ => (Some("X"), theme.text()),
        },
        Family::PlayStation => match b {
            Button::South => (None, theme.role(Role::Accent)),
            Button::East => (None, theme.role(Role::Danger)),
            Button::West => (None, theme.role(Role::Violet)),
            _ => (None, theme.role(Role::Success)),
        },
        Family::Generic | Family::Keyboard => (None, theme.text()),
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
        Button::L1 => "PgUp",
        Button::R1 => "PgDn",
        _ => "",
    }
}

/// A shoulder's or trigger's name as `family` prints it.
fn shoulder(family: Family, b: Button) -> &'static str {
    match (family, b) {
        (Family::PlayStation, Button::L1) => "L1",
        (Family::PlayStation, Button::R1) => "R1",
        (Family::Xbox, Button::L1) => "LB",
        (Family::Xbox, Button::R1) => "RB",
        (Family::Xbox, Button::L2) => "LT",
        (Family::Xbox, Button::R2) => "RT",
        (Family::Nintendo, Button::L2) => "ZL",
        (Family::Nintendo, Button::R2) => "ZR",
        (Family::Nintendo, Button::Start) => "+",
        (Family::Nintendo, Button::Select) => "−",
        (_, Button::L1) => "L",
        (_, Button::R1) => "R",
        (_, Button::L2) => "L2",
        (_, Button::R2) => "R2",
        _ => "•",
    }
}

/// Whether `b` is drawn as a keycap (a label in a square) rather than a
/// round button.
fn is_cap(family: Family, b: Button) -> bool {
    family == Family::Keyboard
        || match b {
            Button::South | Button::East | Button::West | Button::North | Button::Guide => false,
            Button::Start | Button::Select => family == Family::Nintendo,
            _ => true,
        }
}

fn cap_label(family: Family, b: Button) -> &'static str {
    if family == Family::Keyboard { key_for(b) } else { shoulder(family, b) }
}

/// How wide button `b`'s glyph is at radius `r`.
pub fn glyph_width(theme: &Theme, family: Family, b: Button, r: f64) -> f64 {
    if is_cap(family, b) {
        (measure(theme, cap_label(family, b), mono(r * 0.8, MEDIUM)).width + r * 1.1).max(r * 2.0)
    } else {
        r * 2.0
    }
}

/// Button `b` as the controller prints it, `r` its radius: a round
/// button on a raised face, or a keycap.
pub fn button_glyph(c: &mut Canvas, theme: &Theme, family: Family, b: Button, at: Point, r: f64) {
    let edge = (r / 11.0).max(EDGE).round();
    if is_cap(family, b) {
        let w = glyph_width(theme, family, b, r);
        let cap = Rect::new(at.x - w / 2.0, at.y - r * 0.85, at.x + w / 2.0, at.y + r * 0.85);
        let cap = Rect::new(cap.x0.round(), cap.y0.round(), cap.x1.round(), cap.y1.round());
        c.fill_rect(cap, theme.raised());
        border(c, cap, theme.line2(), edge);
        text_centred(c, theme, cap_label(family, b), at, mono(r * 0.8, MEDIUM), theme.text());
        return;
    }
    c.fill(&Circle::new(at, r), theme.raised());
    c.stroke(&Circle::new(at, r - edge / 2.0), &StrokeStyle::new(edge), theme.line2());
    match b {
        Button::South | Button::East | Button::West | Button::North => {
            let (letter, ink) = face_print(theme, family, b);
            if let Some(l) = letter {
                text_centred(c, theme, l, at + Vec2::new(0.0, r * 0.03), font(r * 1.05, BOLD), ink);
            } else if family == Family::PlayStation {
                let s = StrokeStyle::new(r * 0.13).with_cap(LineCap::Round);
                let k = r * 0.42;
                match b {
                    Button::South => {
                        let mut x = BezPath::new();
                        x.move_to((at.x - k, at.y - k));
                        x.line_to((at.x + k, at.y + k));
                        x.move_to((at.x + k, at.y - k));
                        x.line_to((at.x - k, at.y + k));
                        c.stroke(&x, &s, ink);
                    }
                    Button::East => c.stroke(&Circle::new(at, k * 1.05), &s, ink),
                    Button::West => {
                        c.stroke(&Rect::new(at.x - k * 0.9, at.y - k * 0.9, at.x + k * 0.9, at.y + k * 0.9), &s, ink)
                    }
                    _ => {
                        let mut t = BezPath::new();
                        t.move_to((at.x, at.y - k * 1.05));
                        t.line_to((at.x + k * 1.05, at.y + k * 0.75));
                        t.line_to((at.x - k * 1.05, at.y + k * 0.75));
                        t.close_path();
                        c.stroke(&t, &s, ink);
                    }
                }
            } else {
                // The diamond: the button meant filled, the others hollow.
                let k = r * 0.44;
                let dot = r * 0.19;
                for (bb, dx, dy) in [
                    (Button::North, 0.0, -1.0),
                    (Button::East, 1.0, 0.0),
                    (Button::South, 0.0, 1.0),
                    (Button::West, -1.0, 0.0),
                ] {
                    let p = at + Vec2::new(dx * k, dy * k);
                    if bb == b {
                        c.fill(&Circle::new(p, dot * 1.2), theme.focus());
                    } else {
                        c.stroke(&Circle::new(p, dot), &StrokeStyle::new(r * 0.07), theme.text3());
                    }
                }
            }
        }
        // Drawn, not lettered: Xbox's menu lines and view squares, a house
        // for Home.
        Button::Start => icon(c, Icon::Menu, at, r * 1.15, theme.text()),
        Button::Select => icon(c, Icon::Copy, at, r * 1.05, theme.text()),
        _ => icon(c, Icon::House, at, r * 1.1, theme.text()),
    }
}

/// A hint along the foot: a button and what it does. Returns where the
/// next starts.
pub fn hint(c: &mut Canvas, theme: &Theme, family: Family, b: Button, what: &str, x: f64, y: f64) -> f64 {
    let r = 22.0;
    let w = glyph_width(theme, family, b, r);
    button_glyph(c, theme, family, b, Point::new(x + w / 2.0, y), r);
    let tw = text_mid(c, theme, what, x + w + 14.0, y, font(SMALL + 2.0, REGULAR), theme.text2());
    x + w + 14.0 + tw + 44.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_are_where_each_maker_prints_them() {
        let t = Theme::night();
        assert_eq!(face_print(&t, Family::Xbox, Button::South).0, Some("A"));
        assert_eq!(face_print(&t, Family::Nintendo, Button::East).0, Some("A"));
        assert_eq!(face_print(&t, Family::Nintendo, Button::South).0, Some("B"));
        assert_eq!(face_print(&t, Family::PlayStation, Button::South).0, None);
        assert!(is_cap(Family::Xbox, Button::L1));
        assert!(!is_cap(Family::Xbox, Button::Start));
        assert!(is_cap(Family::Nintendo, Button::Start), "Nintendo prints +");
    }
}
