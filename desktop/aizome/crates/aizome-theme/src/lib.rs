//! Aizome's design tokens, as settled on the design canvas and stashed in
//! the `aizome-design` repository (`tokens/aizome-tokens.json`).
//!
//! Everything that draws Aizome — the compositor's tabs and frames, the
//! shell, the kit and every app — takes its colours, metrics and type from
//! here, so a theme or focus colour changes everywhere at once.
//!
//! Colours are sRGB with straight (unpremultiplied) alpha. Metrics are in
//! points: 1pt is 1, 2 or 3 device pixels at Aizome's integer scales.

use serde::{Deserialize, Serialize};

/// An sRGB colour with straight alpha, 0–255 per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// An opaque colour from `0xRRGGBB`.
    pub const fn hex(rgb: u32) -> Rgba {
        Rgba { r: (rgb >> 16) as u8, g: (rgb >> 8) as u8, b: rgb as u8, a: 255 }
    }

    /// A colour from `0xRRGGBB` and an alpha of 0.0–1.0.
    pub const fn hexa(rgb: u32, alpha: f32) -> Rgba {
        let a = (alpha * 255.0 + 0.5) as u8;
        Rgba { r: (rgb >> 16) as u8, g: (rgb >> 8) as u8, b: rgb as u8, a }
    }

    pub const fn with_alpha(self, alpha: f32) -> Rgba {
        Rgba { a: (alpha * 255.0 + 0.5) as u8, ..self }
    }

    /// Channels as 0.0–1.0, straight alpha.
    pub fn to_f32(self) -> [f32; 4] {
        [self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, self.a as f32 / 255.0]
    }

    /// Channels as 0.0–1.0 with the colour multiplied by alpha, as GPU
    /// compositors want them.
    pub fn to_premultiplied_f32(self) -> [f32; 4] {
        let [r, g, b, a] = self.to_f32();
        [r * a, g * a, b * a, a]
    }

    /// Mixes towards `other` by `t` (0.0 is self, 1.0 is other), in sRGB.
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Rgba { r: l(self.r, other.r), g: l(self.g, other.g), b: l(self.b, other.b), a: l(self.a, other.a) }
    }

    /// `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Option<Rgba> {
        let s = s.strip_prefix('#')?;
        let v = u32::from_str_radix(s, 16).ok()?;
        match s.len() {
            6 => Some(Rgba::hex(v)),
            8 => Some(Rgba { r: (v >> 24) as u8, g: (v >> 16) as u8, b: (v >> 8) as u8, a: v as u8 }),
            _ => None,
        }
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f32 {
        let c = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * c(self.r) + 0.7152 * c(self.g) + 0.0722 * c(self.b)
    }

    /// WCAG contrast ratio between two opaque colours.
    pub fn contrast(self, other: Rgba) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

/// Which palette is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    /// Tokyo Night, Tatami's palette: the default.
    #[default]
    Night,
    /// The light theme, a nod to IRIX.
    Day,
    /// Night with stronger edges and text, for low vision.
    HighContrast,
}

/// The colour of the focused window's tab, and of focus everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FocusColor {
    /// Kohaku 琥珀, the default.
    #[default]
    Amber,
    Cyan,
    Rose,
    Lime,
}

/// How much the desktop spends on effects. Aizome must look finished in
/// all three.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effects {
    /// Frosted menus and the Deskbar, 120ms motion.
    Full,
    /// No frost, hard shadows, 80ms motion: the default.
    #[default]
    Lean,
    /// No shadows, no motion, no translucency.
    Austere,
}

impl Effects {
    /// How long a layer takes to move, in milliseconds; zero means it
    /// jumps. Reduced motion forces zero.
    pub fn motion_ms(self) -> u32 {
        match self {
            Effects::Full => 120,
            Effects::Lean => 80,
            Effects::Austere => 0,
        }
    }

    pub fn shadows(self) -> bool {
        self != Effects::Austere
    }
}

/// Every colour a surface of Aizome uses, for one theme and focus colour.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: ThemeName,
    /// Global menu bar and the Deskbar header.
    pub bar: Rgba,
    /// Window content.
    pub surface: Rgba,
    /// Toolbars, the Deskbar, window chrome.
    pub chrome: Rgba,
    /// Menus, torn-off palettes, stacked tabs.
    pub raised: Rgba,
    /// Wells: notification previews, sidebars.
    pub sunk: Rgba,
    /// Dividers and menu rules.
    pub line: Rgba,
    /// Control borders.
    pub line2: Rgba,
    /// 1pt top highlight on frames, bars and tabs.
    pub bevel_hi: Rgba,
    /// 1pt outer edge of frames, tabs and menus.
    pub bevel_lo: Rgba,
    /// The hard 3pt offset shadow: a solid fill, never blurred.
    pub slab: Rgba,
    pub text: Rgba,
    pub text2: Rgba,
    /// Tertiary text, shortcuts, labels.
    pub text3: Rgba,
    /// Decoration only (disabled, line numbers); under 4.5:1.
    pub faint: Rgba,
    /// Tabs of windows without focus.
    pub tab_idle: Rgba,
    pub tab_idle_text: Rgba,
    /// Selected rows and text: ai, the indigo.
    pub selection: Rgba,
    pub on_selection: Rgba,
    /// Title strip of NeXT-style menus and palettes.
    pub menu_title: Rgba,
    pub on_menu_title: Rgba,
    /// Chip behind desktop icon labels.
    pub desktop_label: Rgba,
    /// The focused window's tab, and focus rings.
    pub focus: Rgba,
    pub on_focus: Rgba,
    pub blue: Rgba,
    pub cyan: Rgba,
    pub green: Rgba,
    pub red: Rgba,
    pub violet: Rgba,
    pub amber: Rgba,
    /// The desktop behind everything when no wallpaper shows.
    pub desktop: Rgba,
}

impl Palette {
    pub fn new(name: ThemeName, focus: FocusColor) -> Palette {
        let mut p = match name {
            ThemeName::Night => NIGHT,
            ThemeName::Day => DAY,
            ThemeName::HighContrast => HIGH_CONTRAST,
        };
        p.focus = focus_color(name, focus);
        p
    }

    pub fn night() -> Palette {
        NIGHT
    }

    pub fn day() -> Palette {
        DAY
    }

    pub fn is_dark(&self) -> bool {
        self.name != ThemeName::Day
    }
}

/// The focus colour for a theme.
pub fn focus_color(theme: ThemeName, focus: FocusColor) -> Rgba {
    let day = theme == ThemeName::Day;
    match (focus, day) {
        (FocusColor::Amber, false) => Rgba::hex(0xe0af68),
        (FocusColor::Amber, true) => Rgba::hex(0xf5c542),
        (FocusColor::Cyan, false) => Rgba::hex(0x7dcfff),
        (FocusColor::Cyan, true) => Rgba::hex(0x8fd8f7),
        (FocusColor::Rose, false) => Rgba::hex(0xf7768e),
        (FocusColor::Rose, true) => Rgba::hex(0xffa3b6),
        (FocusColor::Lime, false) => Rgba::hex(0x9ece6a),
        (FocusColor::Lime, true) => Rgba::hex(0xb8e07a),
    }
}

pub const NIGHT: Palette = Palette {
    name: ThemeName::Night,
    bar: Rgba::hex(0x1c1e2c),
    surface: Rgba::hex(0x1a1b26),
    chrome: Rgba::hex(0x222436),
    raised: Rgba::hex(0x2a2e44),
    sunk: Rgba::hex(0x16161e),
    line: Rgba::hex(0x2f334d),
    line2: Rgba::hex(0x3b4261),
    bevel_hi: Rgba::hexa(0xffffff, 0.07),
    bevel_lo: Rgba::hex(0x0b0c12),
    slab: Rgba::hexa(0x000000, 0.5),
    text: Rgba::hex(0xc0caf5),
    text2: Rgba::hex(0xa9b1d6),
    text3: Rgba::hex(0x939aba),
    faint: Rgba::hex(0x565f89),
    tab_idle: Rgba::hex(0x2f334d),
    tab_idle_text: Rgba::hex(0xa9b1d6),
    selection: Rgba::hex(0x3d59a1),
    on_selection: Rgba::hex(0xeef1ff),
    menu_title: Rgba::hex(0x0b0c12),
    on_menu_title: Rgba::hex(0xc0caf5),
    desktop_label: Rgba::hexa(0x0a0a10, 0.62),
    focus: Rgba::hex(0xe0af68),
    on_focus: Rgba::hex(0x1a1b26),
    blue: Rgba::hex(0x7aa2f7),
    cyan: Rgba::hex(0x7dcfff),
    green: Rgba::hex(0x9ece6a),
    red: Rgba::hex(0xf7768e),
    violet: Rgba::hex(0xbb9af7),
    amber: Rgba::hex(0xe0af68),
    desktop: Rgba::hex(0x16161e),
};

pub const DAY: Palette = Palette {
    name: ThemeName::Day,
    bar: Rgba::hex(0xd6d9e8),
    surface: Rgba::hex(0xf4f5fa),
    chrome: Rgba::hex(0xdde0ec),
    raised: Rgba::hex(0xfbfbfe),
    sunk: Rgba::hex(0xe8eaf2),
    line: Rgba::hex(0xb8bdd6),
    line2: Rgba::hex(0x9aa0c2),
    bevel_hi: Rgba::hex(0xffffff),
    bevel_lo: Rgba::hex(0x6f7598),
    slab: Rgba::hexa(0x282c50, 0.32),
    text: Rgba::hex(0x1a1b26),
    text2: Rgba::hex(0x2e3452),
    text3: Rgba::hex(0x50577a),
    faint: Rgba::hex(0x8990b3),
    tab_idle: Rgba::hex(0xc4c8dc),
    tab_idle_text: Rgba::hex(0x2e3452),
    selection: Rgba::hex(0x3d59a1),
    on_selection: Rgba::hex(0xffffff),
    menu_title: Rgba::hex(0x1a1b26),
    on_menu_title: Rgba::hex(0xf4f5fa),
    desktop_label: Rgba::hexa(0xffffff, 0.78),
    focus: Rgba::hex(0xf5c542),
    on_focus: Rgba::hex(0x1a1b26),
    blue: Rgba::hex(0x2e5fd0),
    cyan: Rgba::hex(0x0b6f9c),
    green: Rgba::hex(0x4f7a28),
    red: Rgba::hex(0xc4314b),
    violet: Rgba::hex(0x7247c9),
    amber: Rgba::hex(0x9a6a12),
    desktop: Rgba::hex(0xc9cde0),
};

/// Night with every edge and text colour pushed to at least 7:1, and
/// the tertiary greys raised to the secondary.
pub const HIGH_CONTRAST: Palette = Palette {
    name: ThemeName::HighContrast,
    bar: Rgba::hex(0x0b0c12),
    surface: Rgba::hex(0x0f1018),
    chrome: Rgba::hex(0x14151f),
    raised: Rgba::hex(0x1c1e2c),
    sunk: Rgba::hex(0x08080c),
    line: Rgba::hex(0x7a83b0),
    line2: Rgba::hex(0xa9b1d6),
    bevel_hi: Rgba::hexa(0xffffff, 0.25),
    bevel_lo: Rgba::hex(0xc0caf5),
    slab: Rgba::hex(0x000000),
    text: Rgba::hex(0xffffff),
    text2: Rgba::hex(0xdfe4ff),
    text3: Rgba::hex(0xc0caf5),
    faint: Rgba::hex(0x939aba),
    tab_idle: Rgba::hex(0x2f334d),
    tab_idle_text: Rgba::hex(0xffffff),
    selection: Rgba::hex(0x4a67b3),
    on_selection: Rgba::hex(0xffffff),
    menu_title: Rgba::hex(0x000000),
    on_menu_title: Rgba::hex(0xffffff),
    desktop_label: Rgba::hexa(0x000000, 0.85),
    focus: Rgba::hex(0xffc777),
    on_focus: Rgba::hex(0x000000),
    blue: Rgba::hex(0xa9c4ff),
    cyan: Rgba::hex(0xa4e2ff),
    green: Rgba::hex(0xc3e88d),
    red: Rgba::hex(0xff98a8),
    violet: Rgba::hex(0xd7c2ff),
    amber: Rgba::hex(0xffc777),
    desktop: Rgba::hex(0x000000),
};

/// The mark's colours. The amber is fixed: it does not follow focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brand {
    pub amber: Rgba,
    pub pale: Rgba,
    pub mid: Rgba,
    pub deep: Rgba,
    pub wordmark: Rgba,
    pub seal: Rgba,
}

impl Brand {
    pub fn for_theme(theme: ThemeName) -> Brand {
        match theme {
            ThemeName::Day => Brand {
                amber: Rgba::hex(0xb8790f),
                pale: Rgba::hex(0xa9c4ff),
                mid: Rgba::hex(0x5a7fd6),
                deep: Rgba::hex(0x3d59a1),
                wordmark: Rgba::hex(0x1a1b26),
                seal: Rgba::hex(0x47578f),
            },
            _ => Brand {
                amber: Rgba::hex(0xe0af68),
                pale: Rgba::hex(0xa9c4ff),
                mid: Rgba::hex(0x7aa2f7),
                deep: Rgba::hex(0x4a67b3),
                wordmark: Rgba::hex(0xc0caf5),
                seal: Rgba::hex(0x939aba),
            },
        }
    }

    /// The mark's rectangles in modules (m), in a 4m × 4m box with y down:
    /// the tab (2m × 1m, amber) and three 1m stripes (pale, mid, deep).
    /// Multiply by the module to draw; at 16px the module is 4px.
    pub fn mark_rects(&self) -> [([f32; 4], Rgba); 4] {
        [
            ([0.0, 0.0, 2.0, 1.0], self.amber),
            ([0.0, 1.0, 4.0, 1.0], self.pale),
            ([0.0, 2.0, 4.0, 1.0], self.mid),
            ([0.0, 3.0, 4.0, 1.0], self.deep),
        ]
    }
}

/// The dye ramp, with the names the indigos carry.
pub const DYE_RAMP: [(&str, &str, Rgba); 6] = [
    ("kamenozoki", "甕覗", Rgba::hex(0xa9c4ff)),
    ("hanada", "縹", Rgba::hex(0x7aa2f7)),
    ("ai", "藍", Rgba::hex(0x3d59a1)),
    ("kon", "紺", Rgba::hex(0x24283b)),
    ("kachi", "褐", Rgba::hex(0x16161e)),
    ("kohaku", "琥珀", Rgba::hex(0xe0af68)),
];

/// Metrics, in points.
pub mod metrics {
    /// The global menu bar.
    pub const BAR_HEIGHT: f64 = 24.0;
    /// A window's title tab.
    pub const TAB_HEIGHT: f64 = 22.0;
    /// Horizontal padding inside a tab, each side of the title.
    pub const TAB_PAD: f64 = 10.0;
    /// The narrowest a tab gets (for a short or empty title).
    pub const TAB_MIN_WIDTH: f64 = 64.0;
    /// A row in a menu.
    pub const MENU_ROW: f64 = 24.0;
    /// The smallest pointer target anywhere.
    pub const TARGET: f64 = 24.0;
    /// A window frame's edge.
    pub const FRAME_EDGE: f64 = 1.0;
    pub const BEVEL: f64 = 1.0;
    /// The rail along the top of a focused frame, in the focus colour.
    pub const FOCUS_RAIL: f64 = 2.0;
    /// The hard shadow's offset, right and down.
    pub const SHADOW: f64 = 3.0;
    /// The Deskbar's width.
    pub const DESKBAR_WIDTH: f64 = 232.0;
    /// Everything sits on a 4pt grid.
    pub const GRID: f64 = 4.0;
    /// How far outside a frame its edges can still be grabbed to resize.
    pub const RESIZE_REACH: f64 = 6.0;
}

/// Type families and sizes, in points.
pub mod typeface {
    /// Interface text.
    pub const UI: &str = "Archivo";
    /// Archivo's width in tabs and menu titles: semi-condensed.
    pub const TAB_WIDTH: f32 = 0.875;
    /// Shortcuts, code, meters and clocks (with tabular digits).
    pub const MONO: &str = "IBM Plex Mono";
    /// Kanji only: the seal and dye names.
    pub const KANJI: &str = "Noto Serif CJK JP";
    /// Modifier glyphs ❖ ⇧ ⌥ ⌃, first in every fallback list.
    pub const SYMBOLS: &str = "Noto Sans Symbols 2";

    pub const BODY: f64 = 13.0;
    pub const SMALL: f64 = 11.0;
    pub const TAB: f64 = 12.0;
    pub const MENU: f64 = 13.0;
    pub const TITLE: f64 = 15.0;
    pub const HEADLINE: f64 = 20.0;
    pub const DISPLAY: f64 = 28.0;

    pub const WEIGHT_REGULAR: f32 = 400.0;
    pub const WEIGHT_MEDIUM: f32 = 500.0;
    pub const WEIGHT_SEMIBOLD: f32 = 600.0;
    pub const WEIGHT_TAB: f32 = 650.0;
    pub const WEIGHT_BOLD: f32 = 750.0;
}

/// Modifier glyphs, in the order Aizome prints them: ⌃ ⌥ ⇧ ❖.
pub mod glyphs {
    pub const SUPER: &str = "❖";
    pub const SHIFT: &str = "⇧";
    pub const ALT: &str = "⌥";
    pub const CONTROL: &str = "⌃";
    pub const RETURN: &str = "↩";
    pub const TAB: &str = "⇥";
    pub const ESCAPE: &str = "⎋";
    pub const BACKSPACE: &str = "⌫";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_meets_contrast() {
        for p in [NIGHT, DAY, HIGH_CONTRAST] {
            assert!(p.text.contrast(p.surface) >= 7.0, "{:?} text", p.name);
            assert!(p.text3.contrast(p.surface) >= 4.5, "{:?} text3", p.name);
            assert!(p.on_selection.contrast(p.selection) >= 4.5, "{:?} selection", p.name);
            assert!(p.on_focus.contrast(p.focus) >= 4.5, "{:?} focus", p.name);
        }
        for t in [ThemeName::Night, ThemeName::Day] {
            for f in [FocusColor::Amber, FocusColor::Cyan, FocusColor::Rose, FocusColor::Lime] {
                let p = Palette::new(t, f);
                assert!(p.on_focus.contrast(p.focus) >= 4.5, "{t:?} {f:?}");
            }
        }
    }

    #[test]
    fn parses_hex() {
        assert_eq!(Rgba::parse("#e0af68"), Some(Rgba::hex(0xe0af68)));
        assert_eq!(Rgba::parse("#00000080").map(|c| c.a), Some(128));
    }
}
