//! The theme an app draws with: Aizome's palette for the appearance the
//! user chose (`~/.config/aizome/settings.toml`'s `[appearance]`), metrics
//! scaled by the chosen size, and the fonts.
//!
//! Widgets take every colour from here by role or by token, every length
//! from [`Metrics`], and every font from [`Theme::font`], so a change in
//! Settings re-themes every window at once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

pub use aizome_theme::{Effects, FocusColor, Palette, Rgba, ThemeName};
use aizome_theme::{metrics as tokens, typeface};
use sidestep_ui::{Color, Font, TextStyle};

/// The user's appearance settings: the `[appearance]` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Appearance {
    pub theme: ThemeChoice,
    pub focus: FocusColor,
    pub effects: Effects,
    pub size: SizeChoice,
    pub reduced_motion: bool,
    /// With Auto: Day from sunrise to sunset at `place` rather than from
    /// 07:00 to 19:00.
    pub follow_sunset: bool,
    /// Where sunrise and sunset are reckoned for, in hundredths of a degree
    /// north and east (`aizome_config::Settings::place`); none where it
    /// isn't known or isn't followed.
    pub place: Option<(i32, i32)>,
}

impl Default for Appearance {
    fn default() -> Appearance {
        Appearance {
            theme: ThemeChoice::Night,
            focus: FocusColor::Amber,
            effects: Effects::Lean,
            size: SizeChoice::Medium,
            reduced_motion: false,
            follow_sunset: false,
            place: None,
        }
    }
}

/// Which palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeChoice {
    #[default]
    Night,
    Day,
    HighContrast,
    /// Day from 07:00 to 19:00 local time, or from sunrise to sunset where
    /// it follows them; Night otherwise.
    Auto,
}

/// How large the interface is: it scales layout metrics and type, not
/// pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SizeChoice {
    Small,
    #[default]
    Medium,
    Large,
}

impl SizeChoice {
    pub fn factor(self) -> f64 {
        match self {
            SizeChoice::Small => 0.9,
            SizeChoice::Medium => 1.0,
            SizeChoice::Large => 1.15,
        }
    }
}

impl From<&aizome_config::Appearance> for Appearance {
    fn from(a: &aizome_config::Appearance) -> Appearance {
        use aizome_config::{ThemeChoice as T, UiSize as S};
        Appearance {
            theme: match a.theme {
                T::Night => ThemeChoice::Night,
                T::Day => ThemeChoice::Day,
                T::HighContrast => ThemeChoice::HighContrast,
                T::Auto => ThemeChoice::Auto,
            },
            focus: a.focus,
            effects: a.effects,
            size: match a.size {
                S::Small => SizeChoice::Small,
                S::Medium => SizeChoice::Medium,
                S::Large => SizeChoice::Large,
            },
            reduced_motion: a.reduced_motion,
            follow_sunset: a.follow_sunset,
            place: None,
        }
    }
}

impl From<&aizome_config::Settings> for Appearance {
    /// The `[appearance]` table, with the place sunrise and sunset are
    /// reckoned for when Auto follows them.
    fn from(s: &aizome_config::Settings) -> Appearance {
        let follows = s.appearance.theme == aizome_config::ThemeChoice::Auto && s.appearance.follow_sunset;
        let place = if follows { s.place() } else { None };
        Appearance {
            place: place.map(|p| ((p.latitude * 100.0).round() as i32, (p.longitude * 100.0).round() as i32)),
            ..Appearance::from(&s.appearance)
        }
    }
}

impl Appearance {
    /// The `[appearance]` table of a settings file's text (through
    /// `aizome-config`); a broken file gives the defaults.
    pub fn parse(toml_text: &str) -> Appearance {
        aizome_config::Settings::from_toml(toml_text).map(|s| Appearance::from(&s)).unwrap_or_default()
    }

    /// The settings file's appearance, or the defaults if there's none.
    pub fn load() -> Appearance {
        Appearance::from(&aizome_config::Settings::load_or_default())
    }

    /// The palette for these settings now.
    pub fn theme_name(&self) -> ThemeName {
        self.theme_at(aizome_config::LocalNow::now())
    }

    /// The palette at a moment: Auto shows Day from sunrise to sunset at
    /// the place where it follows them, else from 07:00 to 19:00.
    pub fn theme_at(&self, now: aizome_config::LocalNow) -> ThemeName {
        let day = match (self.follow_sunset, self.place) {
            (true, Some((lat, lon))) => {
                let place = aizome_config::Place {
                    latitude: f64::from(lat) / 100.0,
                    longitude: f64::from(lon) / 100.0,
                    source: aizome_config::PlaceSource::Settings,
                };
                aizome_config::daylight(Some(&place), now.day_of_year, now.utc_offset)
                    .unwrap_or(aizome_config::CLOCK_DAY)
            }
            _ => aizome_config::CLOCK_DAY,
        };
        let choice = match self.theme {
            ThemeChoice::Night => aizome_config::ThemeChoice::Night,
            ThemeChoice::Day => aizome_config::ThemeChoice::Day,
            ThemeChoice::HighContrast => aizome_config::ThemeChoice::HighContrast,
            ThemeChoice::Auto => aizome_config::ThemeChoice::Auto,
        };
        choice.resolve_in(day, now.minutes)
    }
}

/// The hour of the day where the user is (0–23).
pub fn local_hour() -> u32 {
    // SAFETY: `localtime_r` writes only into the `tm` given.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 12;
        }
        tm.tm_hour.clamp(0, 23) as u32
    }
}

/// The settings file: `$XDG_CONFIG_HOME/aizome/settings.toml`, else
/// `~/.config/aizome/settings.toml`.
pub fn settings_path() -> PathBuf {
    aizome_config::Settings::path()
}

/// Lengths, in points, at the chosen size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The size's factor (1.0 at medium).
    pub factor: f64,
    /// Body text.
    pub body: f64,
    /// Secondary text, captions.
    pub small: f64,
    /// Section captions (mono, uppercase).
    pub caption: f64,
    pub title: f64,
    pub headline: f64,
    pub display: f64,
    /// Shortcuts beside menu items.
    pub shortcut: f64,
    /// Buttons, fields, pop-ups: the height of a control.
    pub control: f64,
    /// A control drawn inside a toolbar (its target stays `control` tall).
    pub control_compact: f64,
    /// A row of a list, a menu, a sidebar.
    pub row: f64,
    pub menu_row: f64,
    pub toolbar: f64,
    pub status_bar: f64,
    /// A settings form's row.
    pub form_row: f64,
    /// Padding inside controls, left and right.
    pub pad: f64,
    /// Space between things side by side.
    pub gap: f64,
    /// The usual icon size.
    pub icon: f64,
    pub sidebar: f64,
    /// The smallest pointer target.
    pub target: f64,
    pub bevel: f64,
    pub shadow: f64,
    pub focus_ring: f64,
}

impl Metrics {
    pub fn new(size: SizeChoice) -> Metrics {
        let f = size.factor();
        // Type to the half point, lengths to the point, targets never
        // under 24pt.
        let t = |v: f64| (v * f * 2.0).round() / 2.0;
        let l = |v: f64| (v * f).round();
        let target = tokens::TARGET;
        Metrics {
            factor: f,
            body: t(typeface::BODY),
            small: t(typeface::SMALL),
            caption: t(10.5),
            title: t(typeface::TITLE),
            headline: t(typeface::HEADLINE),
            display: t(typeface::DISPLAY),
            shortcut: t(11.5),
            control: l(24.0).max(target),
            control_compact: l(20.0),
            row: l(24.0).max(target),
            menu_row: l(tokens::MENU_ROW).max(target),
            toolbar: l(30.0).max(target + 6.0),
            status_bar: l(22.0),
            form_row: l(36.0),
            pad: l(8.0),
            gap: l(8.0),
            icon: l(16.0),
            sidebar: l(200.0),
            target,
            bevel: tokens::BEVEL,
            shadow: tokens::SHADOW,
            focus_ring: 2.0,
        }
    }
}

/// A colour by what it's for, so text follows the theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Role {
    #[default]
    Text,
    Text2,
    Text3,
    /// Decoration only (disabled text, line numbers).
    Faint,
    /// Links, headings, the hanada blue.
    Accent,
    Danger,
    Success,
    Warning,
    Violet,
    Cyan,
    /// Text on the selection fill.
    OnSelection,
    /// Text on the focus colour.
    OnFocus,
    /// The focus colour itself.
    Focus,
}

/// Which family text is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    /// Archivo.
    #[default]
    Ui,
    /// IBM Plex Mono.
    Mono,
    /// Noto Sans Symbols 2: ❖ ⇧ ⌥ ⌃.
    Symbols,
}

/// What a font is asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSpec {
    pub family: Family,
    pub size: f64,
    pub weight: f32,
    pub italic: bool,
    pub tabular: bool,
    /// Archivo at 87.5% width, as tabs and menu titles set it.
    pub condensed: bool,
}

impl FontSpec {
    pub fn ui(size: f64) -> FontSpec {
        FontSpec {
            family: Family::Ui,
            size,
            weight: typeface::WEIGHT_REGULAR,
            italic: false,
            tabular: false,
            condensed: false,
        }
    }

    pub fn mono(size: f64) -> FontSpec {
        FontSpec { family: Family::Mono, ..FontSpec::ui(size) }
    }

    fn key(&self) -> (Family, u64, u32, bool, bool, bool) {
        (self.family, self.size.to_bits(), self.weight.to_bits(), self.italic, self.tabular, self.condensed)
    }
}

/// Fonts made, by what was asked: resolving a family through fontconfig
/// is costly, so each is made once.
#[derive(Default)]
struct Fonts {
    made: HashMap<(Family, u64, u32, bool, bool, bool), Font>,
    /// Whether each family is there (else the system's sans or mono).
    present: HashMap<Family, bool>,
}

/// Everything a widget draws with.
#[derive(Clone)]
pub struct Theme {
    pub palette: Palette,
    pub appearance: Appearance,
    pub metrics: Metrics,
    /// Bumped on every change, so caches keyed by it start over.
    pub generation: u64,
    fonts: Rc<RefCell<Fonts>>,
}

impl std::fmt::Debug for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Theme").field("palette", &self.palette.name).field("appearance", &self.appearance).finish()
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::new(Appearance::default())
    }
}

/// An Aizome colour as Sidestep's.
pub fn color(c: Rgba) -> Color {
    Color::rgba8(c.r, c.g, c.b, c.a)
}

impl Theme {
    pub fn new(appearance: Appearance) -> Theme {
        let name = appearance.theme_name();
        Theme {
            palette: Palette::new(name, appearance.focus),
            appearance,
            metrics: Metrics::new(appearance.size),
            generation: 0,
            fonts: Rc::default(),
        }
    }

    /// Night (the default), with every other setting at its default.
    pub fn night() -> Theme {
        Theme::default()
    }

    pub fn day() -> Theme {
        Theme::new(Appearance { theme: ThemeChoice::Day, ..Appearance::default() })
    }

    pub fn high_contrast() -> Theme {
        Theme::new(Appearance { theme: ThemeChoice::HighContrast, ..Appearance::default() })
    }

    /// The same fonts, with new settings.
    pub(crate) fn rethemed(&self, appearance: Appearance) -> Theme {
        let mut t = Theme::new(appearance);
        t.fonts = self.fonts.clone();
        t.generation = self.generation + 1;
        t
    }

    pub fn is_dark(&self) -> bool {
        self.palette.is_dark()
    }

    /// Whether things may move (effects and reduced motion allowing).
    pub fn motion(&self) -> bool {
        !self.appearance.reduced_motion && self.appearance.effects.motion_ms() > 0
    }

    /// Whether floating things cast their hard shadow.
    pub fn shadows(&self) -> bool {
        self.appearance.effects.shadows()
    }

    /// Whether menus and floating panels are frosted (the Full effects):
    /// a translucent face with a fine grain, the hard shadow kept.
    pub fn frosted(&self) -> bool {
        self.appearance.effects == Effects::Full
    }

    pub fn role(&self, role: Role) -> Color {
        let p = &self.palette;
        color(match role {
            Role::Text => p.text,
            Role::Text2 => p.text2,
            Role::Text3 => p.text3,
            Role::Faint => p.faint,
            Role::Accent => p.blue,
            Role::Danger => p.red,
            Role::Success => p.green,
            Role::Warning => p.amber,
            Role::Violet => p.violet,
            Role::Cyan => p.cyan,
            Role::OnSelection => p.on_selection,
            Role::OnFocus => p.on_focus,
            Role::Focus => p.focus,
        })
    }

    // Tokens, as Sidestep colours.

    pub fn bar(&self) -> Color {
        color(self.palette.bar)
    }
    pub fn surface(&self) -> Color {
        color(self.palette.surface)
    }
    pub fn chrome(&self) -> Color {
        color(self.palette.chrome)
    }
    pub fn raised(&self) -> Color {
        color(self.palette.raised)
    }
    pub fn sunk(&self) -> Color {
        color(self.palette.sunk)
    }
    pub fn line(&self) -> Color {
        color(self.palette.line)
    }
    pub fn line2(&self) -> Color {
        color(self.palette.line2)
    }
    pub fn bevel_hi(&self) -> Color {
        color(self.palette.bevel_hi)
    }
    pub fn bevel_lo(&self) -> Color {
        color(self.palette.bevel_lo)
    }
    pub fn slab(&self) -> Color {
        color(self.palette.slab)
    }
    pub fn selection(&self) -> Color {
        color(self.palette.selection)
    }
    pub fn on_selection(&self) -> Color {
        color(self.palette.on_selection)
    }
    pub fn focus(&self) -> Color {
        color(self.palette.focus)
    }
    pub fn on_focus(&self) -> Color {
        color(self.palette.on_focus)
    }
    pub fn menu_title(&self) -> Color {
        color(self.palette.menu_title)
    }
    pub fn on_menu_title(&self) -> Color {
        color(self.palette.on_menu_title)
    }
    pub fn tab_idle(&self) -> Color {
        color(self.palette.tab_idle)
    }
    pub fn text(&self) -> Color {
        color(self.palette.text)
    }
    pub fn text2(&self) -> Color {
        color(self.palette.text2)
    }
    pub fn text3(&self) -> Color {
        color(self.palette.text3)
    }
    pub fn faint(&self) -> Color {
        color(self.palette.faint)
    }

    /// A control's face lit by the pointer: a little toward the text.
    pub fn hover(&self, base: Color) -> Color {
        base.mix(self.text(), if self.is_dark() { 0.07 } else { 0.05 })
    }

    /// A control's face held down.
    pub fn pressed(&self, base: Color) -> Color {
        base.mix(self.sunk(), 0.7).mix(color(self.palette.bevel_lo), 0.12)
    }

    // Fonts.

    /// The font for `spec`, made once.
    pub fn font(&self, spec: FontSpec) -> Font {
        let mut fonts = self.fonts.borrow_mut();
        if let Some(f) = fonts.made.get(&spec.key()) {
            return f.clone();
        }
        let present = *fonts.present.entry(spec.family).or_insert_with(|| {
            let name = match spec.family {
                Family::Ui => typeface::UI,
                Family::Mono => typeface::MONO,
                Family::Symbols => typeface::SYMBOLS,
            };
            Font::named(name, 13.0).is_some_and(|f| f.family_name().eq_ignore_ascii_case(name))
        });
        let base = match (spec.family, present) {
            (Family::Ui, true) => Font::named(typeface::UI, spec.size),
            (Family::Mono, true) => Font::named(typeface::MONO, spec.size),
            (Family::Symbols, true) => Font::named(typeface::SYMBOLS, spec.size),
            _ => None,
        };
        let base = base.unwrap_or_else(|| match spec.family {
            Family::Mono => Font::monospace(spec.size),
            _ => Font::system(spec.size),
        });
        let mut font = base.with_weight(spec.weight);
        if spec.italic {
            font = font.italic();
        }
        if spec.tabular {
            font = font.with_tabular_digits();
        }
        if spec.condensed {
            font = font.with_width(typeface::TAB_WIDTH);
        }
        fonts.made.insert(spec.key(), font.clone());
        font
    }

    /// Body text in `role`.
    pub fn body(&self, role: Role) -> TextStyle {
        TextStyle::new(self.font(FontSpec::ui(self.metrics.body)), self.role(role))
    }

    pub fn style(&self, spec: FontSpec, color: Color) -> TextStyle {
        TextStyle::new(self.font(spec), color)
    }

    /// Whether Archivo (not the system's sans) sets interface text.
    pub fn has_ui_font(&self) -> bool {
        let _ = self.font(FontSpec::ui(13.0));
        self.fonts.borrow().present.get(&Family::Ui).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_parses_what_it_knows() {
        let a = Appearance::parse(
            "[appearance]\ntheme = \"day\"\nfocus = \"cyan\"\neffects = \"austere\"\nsize = \"large\"\nreduced_motion = true\n[other]\nx = 1\n",
        );
        assert_eq!(a.theme, ThemeChoice::Day);
        assert_eq!(a.focus, FocusColor::Cyan);
        assert_eq!(a.effects, Effects::Austere);
        assert_eq!(a.size, SizeChoice::Large);
        assert!(a.reduced_motion);
        assert_eq!(Appearance::parse("not toml ["), Appearance::default());
        assert_eq!(Appearance::parse("[appearance]\ntheme = 3\n").theme, ThemeChoice::Night);
        assert_eq!(Appearance::parse("[appearance]\ntheme = \"auto\"\n").theme, ThemeChoice::Auto);
    }

    #[test]
    fn auto_follows_the_sun_where_it_is_asked_to() {
        // Paris, as set by hand, at 18:00 on 1 November: after sunset
        // (about 17:25), before 19:00.
        let text = "[appearance]\ntheme = \"auto\"\nfollow_sunset = true\n[night_light]\nlatitude = 48.8667\nlongitude = 2.3333\n";
        let a = Appearance::parse(text);
        assert_eq!(a.place, Some((4887, 233)));
        let evening = aizome_config::LocalNow::at(18, 0, 305, 60);
        assert_eq!(a.theme_at(evening), ThemeName::Night);
        assert_eq!(a.theme_at(aizome_config::LocalNow::at(12, 0, 305, 60)), ThemeName::Day);
        let clock = Appearance { follow_sunset: false, ..a };
        assert_eq!(clock.theme_at(evening), ThemeName::Day, "by the clock until 19:00");
        // Not following, no place is looked up (the appearance stays the same).
        assert_eq!(Appearance::parse("[appearance]\ntheme = \"auto\"\n").place, None);
    }

    #[test]
    fn sizes_scale_metrics_but_keep_targets() {
        let small = Metrics::new(SizeChoice::Small);
        let large = Metrics::new(SizeChoice::Large);
        assert!(small.body < 13.0 && large.body > 13.0);
        assert!(small.control >= 24.0 && small.row >= 24.0 && small.menu_row >= 24.0);
        assert!(large.control > 24.0);
        assert_eq!(Metrics::new(SizeChoice::Medium).body, 13.0);
    }
}
