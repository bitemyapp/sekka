//! Calendar colours in the theme: each dye as the palette has it in
//! Night, Day and High Contrast, and what reads on it.

use aizome_kit::Color;
use aizome_kit::theme::{Theme, color};
use aizome_theme::{DYE_RAMP, Rgba};

use crate::model::Dye;

/// A dye's colour in `t`.
pub fn dye(t: &Theme, d: Dye) -> Color {
    let p = &t.palette;
    let dark = t.is_dark();
    let ramp = |name: &str| DYE_RAMP.iter().find(|(n, _, _)| *n == name).map(|(_, _, c)| *c).unwrap_or(p.blue);
    color(match d {
        Dye::Hanada => p.blue,
        // In Night the deep indigo needs lifting off the dark ground.
        Dye::Ai => {
            if dark {
                ramp("ai").mix(Rgba::hex(0x7aa2f7), 0.2)
            } else {
                ramp("ai")
            }
        }
        Dye::Kamenozoki => {
            if dark {
                ramp("kamenozoki")
            } else {
                // Day's pale ground needs it a shade deeper.
                ramp("kamenozoki").mix(Rgba::hex(0x3d59a1), 0.35)
            }
        }
        Dye::Cyan => p.cyan,
        Dye::Green => p.green,
        Dye::Red => p.red,
        Dye::Violet => p.violet,
        Dye::Kohaku => {
            if dark {
                ramp("kohaku")
            } else {
                p.amber
            }
        }
    })
}

fn luminance(c: Color) -> f32 {
    let f = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

/// Text on a dye's full colour: the dark ground or white, whichever
/// reads better.
pub fn on_dye(t: &Theme, fill: Color) -> Color {
    let l = luminance(fill);
    let dark = color(aizome_theme::NIGHT.surface);
    let _ = t;
    // Contrast against near-black and white.
    let c_dark = (l + 0.05) / (luminance(dark) + 0.05);
    let c_white = 1.05 / (l + 0.05);
    if c_dark >= c_white { dark } else { Color::WHITE }
}

/// A dye laid lightly over the surface: the face of an event that isn't
/// selected.
pub fn tint(t: &Theme, fill: Color) -> Color {
    let base = t.surface();
    base.mix(fill, if t.is_dark() { 0.24 } else { 0.2 })
}

/// Text on a tint: the theme's text, nudged toward the dye.
pub fn on_tint(t: &Theme, fill: Color) -> Color {
    t.text().mix(fill, if t.is_dark() { 0.12 } else { 0.25 })
}

/// The now-line's amber: Kohaku whatever the focus colour, deep enough to
/// read on Day's pale ground.
pub fn now_line(t: &Theme) -> Color {
    color(t.palette.amber)
}
