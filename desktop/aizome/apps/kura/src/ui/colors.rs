//! The sunburst's colours: the dye ramp (Kura's icon's indigos, kohaku's
//! amber and the cyan beside them) by top-level branch, each ring further
//! out a little lighter, files a little quieter than folders, and grey for
//! the smaller items.

use aizome_kit::{Color, Theme};
use aizome_theme::{DYE_RAMP, Rgba};

fn dye(name: &str) -> Rgba {
    DYE_RAMP.iter().find(|(n, _, _)| *n == name).map_or(Rgba::hex(0x7aa2f7), |(_, _, c)| *c)
}

/// The branch colours, in order: light, amber, deep, pale, cyan, so that
/// neighbours differ.
pub fn ramp(theme: &Theme) -> [Color; 5] {
    let c = aizome_kit::theme::color;
    if theme.is_dark() {
        [c(dye("hanada")), c(dye("kohaku")), c(Rgba::hex(0x4a67b3)), c(dye("kamenozoki")), c(theme.palette.cyan)]
    } else {
        // Day's surface is pale: the same dyes, a shade deeper.
        [c(Rgba::hex(0x5a7fd6)), c(Rgba::hex(0xd99a2b)), c(dye("ai")), c(Rgba::hex(0x8fb0f5)), c(Rgba::hex(0x3fa6d9))]
    }
}

/// A segment's fill: its branch's dye, lighter by ring.
pub fn segment(theme: &Theme, branch: u16, depth: u8, is_dir: bool) -> Color {
    let ramp = ramp(theme);
    let base = ramp[branch as usize % ramp.len()];
    let step = if theme.is_dark() { 0.09 } else { 0.12 };
    let lift = (f32::from(depth.saturating_sub(1)) * step).min(0.5);
    let c = base.mix(Color::WHITE, lift);
    if is_dir { c } else { c.mix(aizome_kit::theme::color(theme.palette.surface), 0.22) }
}

/// The fill of "smaller items".
pub fn small(theme: &Theme, depth: u8) -> Color {
    let base = aizome_kit::theme::color(theme.palette.line2);
    base.mix(Color::WHITE, (f32::from(depth.saturating_sub(1)) * 0.06).min(0.3))
}

/// A segment under the pointer.
pub fn hovered(c: Color) -> Color {
    c.mix(Color::WHITE, 0.3)
}

/// Relative luminance (WCAG), for choosing text over a fill.
pub fn luminance(c: Color) -> f32 {
    let f = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

/// Text that reads on `fill`: Night's ink or white, whichever contrasts more.
pub fn text_on(fill: Color) -> Color {
    let ink = Color::hex(0x1a1b26);
    let l = luminance(fill);
    let on_ink = (l + 0.05) / (luminance(ink) + 0.05);
    let on_white = 1.05 / (l + 0.05);
    if on_ink >= on_white { ink } else { Color::WHITE }
}
