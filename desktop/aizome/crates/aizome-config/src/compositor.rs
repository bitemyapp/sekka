//! Compositor keys that live in the sections' extra tables (Settings
//! writes them there; see Settei's `extra.rs`), read with defaults, and
//! Night Light.

use serde::{Deserialize, Serialize};

use crate::location::{LocalNow, Place, Span, daylight};
use crate::{Keyboard, OutputConfig, Pointer, Settings, Touchpad, Windows};

/// What double-clicking a tab does: `[windows] tab_double_click`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabDoubleClick {
    /// Fill the usable area, or go back (the zoom box).
    #[default]
    Fill,
    /// Park the window in the Deskbar.
    Minimise,
    Nothing,
}

impl Windows {
    pub fn tab_double_click(&self) -> TabDoubleClick {
        match self.extra.get("tab_double_click").and_then(toml::Value::as_str) {
            Some("minimise" | "minimize" | "park") => TabDoubleClick::Minimise,
            Some("nothing" | "none") => TabDoubleClick::Nothing,
            _ => TabDoubleClick::Fill,
        }
    }
}

fn factor(t: &toml::Table) -> f64 {
    let f = match t.get("scroll_factor") {
        Some(toml::Value::Float(f)) => *f,
        Some(toml::Value::Integer(i)) => *i as f64,
        _ => 1.0,
    };
    if f.is_finite() { f.clamp(0.1, 10.0) } else { 1.0 }
}

impl Pointer {
    /// How far a scroll goes, times the device's: `scroll_factor`.
    pub fn scroll_factor(&self) -> f64 {
        factor(&self.extra)
    }
}

impl Touchpad {
    pub fn scroll_factor(&self) -> f64 {
        factor(&self.extra)
    }
}

impl Keyboard {
    /// Sticky keys: a modifier pressed and released alone applies to the
    /// next key.
    pub fn sticky_keys(&self) -> bool {
        self.extra.get("sticky_keys").and_then(toml::Value::as_bool).unwrap_or(false)
    }

    /// Slow keys: a key counts only once held this long (`None` when off).
    pub fn slow_keys(&self) -> Option<u32> {
        if !self.extra.get("slow_keys").and_then(toml::Value::as_bool).unwrap_or(false) {
            return None;
        }
        let ms = self.extra.get("slow_keys_ms").and_then(toml::Value::as_integer).unwrap_or(300);
        Some(ms.clamp(50, 2000) as u32)
    }
}

/// The renderer to use: `[performance] renderer`, read at start-up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RendererChoice {
    /// GLES, falling back to pixman.
    #[default]
    Auto,
    /// GLES.
    Gpu,
    /// Pixman.
    Software,
}

impl Settings {
    pub fn renderer_choice(&self) -> RendererChoice {
        let r = self
            .extra
            .get("performance")
            .and_then(toml::Value::as_table)
            .and_then(|t| t.get("renderer"))
            .and_then(toml::Value::as_str);
        match r {
            Some("gpu" | "gles") => RendererChoice::Gpu,
            Some("software" | "pixman") => RendererChoice::Software,
            _ => RendererChoice::Auto,
        }
    }
}

impl OutputConfig {
    /// The connector this display mirrors (`mirror = "eDP-1"`).
    pub fn mirror(&self) -> Option<&str> {
        self.extra.get("mirror").and_then(toml::Value::as_str).filter(|s| !s.is_empty())
    }

    /// The main display: the Deskbar's, and new windows' when nothing
    /// else decides (`primary = true`).
    pub fn primary(&self) -> bool {
        self.extra.get("primary").and_then(toml::Value::as_bool).unwrap_or(false)
    }
}

/// When Night Light warms the displays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NightSchedule {
    /// From sunset to sunrise where the computer is (`latitude` and
    /// `longitude` when set, else the place the shell found or the time
    /// zone's city: see [`crate::location`]); 19:00 to 07:00 where that's
    /// unknown.
    #[default]
    Sunset,
    /// From `from` to `to`, local time.
    Manual,
    /// All the time it's on.
    Always,
}

/// `[night_light]`: warmer colours at night.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NightLight {
    pub enabled: bool,
    /// The colour temperature while warm, in Kelvin (1900–6500).
    pub temperature: u32,
    pub schedule: NightSchedule,
    /// `"HH:MM"`, for the manual schedule.
    pub from: String,
    pub to: String,
    /// A place set by hand for sunset and sunrise (both, or neither);
    /// unset, Aizome works out where the computer is.
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for NightLight {
    fn default() -> Self {
        NightLight {
            enabled: false,
            temperature: 4000,
            schedule: NightSchedule::Sunset,
            from: "22:00".into(),
            to: "07:00".into(),
            latitude: None,
            longitude: None,
            extra: toml::Table::new(),
        }
    }
}

/// Neutral: no warming.
pub const NEUTRAL_K: u32 = 6500;

/// The sunset schedule's night where Aizome doesn't know where the
/// computer is: 19:00 to 07:00.
pub const CLOCK_NIGHT: Span = Span::Between { start: 19 * 60, end: 7 * 60 };

impl NightLight {
    /// When the schedule warms the displays on a day: from sunset to
    /// sunrise at `place` (the settings' [`Settings::place`]), from `from`
    /// to `to`, or always.
    pub fn span(&self, place: Option<&Place>, day_of_year: u32, utc_offset: i32) -> Span {
        match self.schedule {
            NightSchedule::Always => Span::AllDay,
            NightSchedule::Manual => match (parse_hm(&self.from), parse_hm(&self.to)) {
                (Some(start), Some(end)) => Span::Between { start, end },
                _ => Span::Between { start: 22 * 60, end: 7 * 60 },
            },
            NightSchedule::Sunset => match daylight(place, day_of_year, utc_offset) {
                Some(Span::Between { start, end }) => Span::Between { start: end, end: start },
                Some(Span::AllDay) => Span::Never,
                Some(Span::Never) => Span::AllDay,
                None => CLOCK_NIGHT,
            },
        }
    }

    /// Whether the schedule says warm at a moment.
    pub fn scheduled(&self, place: Option<&Place>, now: LocalNow) -> bool {
        self.span(place, now.day_of_year, now.utc_offset).contains(now.minutes)
    }

    /// The temperature to show at a moment: warm when on and scheduled.
    pub fn temperature_at(&self, place: Option<&Place>, now: LocalNow) -> u32 {
        if self.enabled && self.scheduled(place, now) { self.temperature.clamp(1900, NEUTRAL_K) } else { NEUTRAL_K }
    }
}

/// `"22:00"` as minutes since midnight.
pub fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sun {
    /// Local minutes since midnight.
    Rises {
        sunrise: u32,
        sunset: u32,
    },
    AlwaysUp,
    AlwaysDown,
}

/// Sunrise and sunset, local time (NOAA's approximation, within a few
/// minutes).
pub fn sun_times(latitude: f64, longitude: f64, day_of_year: u32, utc_offset: i32) -> Sun {
    use std::f64::consts::PI;
    let g = 2.0 * PI / 365.0 * (day_of_year.saturating_sub(1) as f64);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let lat = latitude.to_radians();
    let cos_ha = (90.833f64.to_radians().cos()) / (lat.cos() * decl.cos()) - lat.tan() * decl.tan();
    if cos_ha > 1.0 {
        return Sun::AlwaysDown;
    }
    if cos_ha < -1.0 {
        return Sun::AlwaysUp;
    }
    let ha = cos_ha.acos().to_degrees();
    let wrap = |m: f64| (((m + utc_offset as f64).round() as i64).rem_euclid(1440)) as u32;
    Sun::Rises {
        sunrise: wrap(720.0 - 4.0 * (longitude + ha) - eqtime),
        sunset: wrap(720.0 - 4.0 * (longitude - ha) - eqtime),
    }
}

/// A colour temperature as red, green and blue multipliers, 6500 K being
/// 1, 1, 1 (Tanner Helland's fit of the black-body colours).
pub fn temperature_rgb(kelvin: u32) -> [f64; 3] {
    fn raw(k: f64) -> [f64; 3] {
        let t = k / 100.0;
        let r = if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) };
        let g = if t <= 66.0 {
            99.4708025861 * t.ln() - 161.1195681661
        } else {
            288.1221695283 * (t - 60.0).powf(-0.0755148492)
        };
        let b = if t >= 66.0 {
            255.0
        } else if t <= 19.0 {
            0.0
        } else {
            138.5177312231 * (t - 10.0).ln() - 305.0447927307
        };
        [r.clamp(0.0, 255.0), g.clamp(0.0, 255.0), b.clamp(0.0, 255.0)]
    }
    let n = raw(NEUTRAL_K as f64);
    let c = raw(kelvin.clamp(1000, NEUTRAL_K) as f64);
    [(c[0] / n[0]).min(1.0), (c[1] / n[1]).min(1.0), (c[2] / n[2]).min(1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_keys_read_with_defaults() {
        let s = Settings::from_toml(
            "[windows]\ntab_double_click = \"minimise\"\n[input.pointer]\nscroll_factor = 2.5\n[input.keyboard]\nsticky_keys = true\nslow_keys = true\nslow_keys_ms = 450\n[performance]\nrenderer = \"software\"\n[[outputs]]\nname = \"HDMI-A-1\"\nmirror = \"eDP-1\"\nprimary = true\n",
        )
        .unwrap();
        assert_eq!(s.windows.tab_double_click(), TabDoubleClick::Minimise);
        assert_eq!(s.input.pointer.scroll_factor(), 2.5);
        assert_eq!(s.input.touchpad.scroll_factor(), 1.0);
        assert!(s.input.keyboard.sticky_keys());
        assert_eq!(s.input.keyboard.slow_keys(), Some(450));
        assert_eq!(s.renderer_choice(), RendererChoice::Software);
        assert_eq!(s.outputs[0].mirror(), Some("eDP-1"));
        assert!(s.outputs[0].primary());
        let d = Settings::default();
        assert_eq!(d.windows.tab_double_click(), TabDoubleClick::Fill);
        assert_eq!(d.input.keyboard.slow_keys(), None);
        assert_eq!(d.renderer_choice(), RendererChoice::Auto);
        // Written back as they were.
        let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
        assert_eq!(back.outputs[0].mirror(), Some("eDP-1"));
    }

    #[test]
    fn night_light_schedules() {
        let mut n = NightLight {
            enabled: true,
            schedule: NightSchedule::Manual,
            from: "21:30".into(),
            to: "06:15".into(),
            ..Default::default()
        };
        let at = |h: u32| LocalNow::at(h, 0, 100, 0);
        assert!(n.scheduled(None, at(22)));
        assert!(n.scheduled(None, at(3)));
        assert!(!n.scheduled(None, at(12)));
        assert_eq!(n.temperature_at(None, at(23)), 4000);
        assert_eq!(n.temperature_at(None, at(12)), NEUTRAL_K);
        n.enabled = false;
        assert_eq!(n.temperature_at(None, at(23)), NEUTRAL_K);
        // The sunset schedule: 19:00 to 07:00 where the place isn't known,
        // else the sun's.
        n.enabled = true;
        n.schedule = NightSchedule::Sunset;
        assert_eq!(n.span(None, 291, 120), CLOCK_NIGHT);
        let paris =
            Place { latitude: 48.8667, longitude: 2.3333, source: crate::PlaceSource::TimeZone("Europe/Paris".into()) };
        let Span::Between { start, end } = n.span(Some(&paris), 291, 120) else { panic!() };
        assert!((18 * 60 + 45..19 * 60 + 5).contains(&start), "sunset {}", crate::hm(start));
        assert!((8 * 60 + 5..8 * 60 + 25).contains(&end), "sunrise {}", crate::hm(end));
        // 19:10 in Paris in October is night already, and 07:30 still is.
        assert_eq!(n.temperature_at(Some(&paris), LocalNow::at(19, 10, 291, 120)), 4000);
        assert_eq!(n.temperature_at(Some(&paris), LocalNow::at(7, 30, 291, 120)), 4000);
        assert_eq!(n.temperature_at(Some(&paris), LocalNow::at(9, 0, 291, 120)), NEUTRAL_K);
        let north = Place { latitude: 69.65, longitude: 18.96, source: crate::PlaceSource::Settings };
        assert_eq!(n.span(Some(&north), 172, 120), Span::Never, "midsummer in Tromsø");
        // Tokyo at the March equinox: about 05:45 and 17:55.
        let Sun::Rises { sunrise, sunset } = sun_times(35.68, 139.69, 80, 9 * 60) else { panic!() };
        assert!((5 * 60 + 30..6 * 60).contains(&sunrise), "{sunrise}");
        assert!((17 * 60 + 40..18 * 60 + 10).contains(&sunset), "{sunset}");
        // Tromsø in June: the sun doesn't set.
        assert_eq!(sun_times(69.65, 18.96, 172, 120), Sun::AlwaysUp);
        assert_eq!(parse_hm("7:05"), Some(425));
        assert_eq!(parse_hm("24:00"), None);
    }

    #[test]
    fn temperatures_warm_the_blue_end() {
        assert_eq!(temperature_rgb(6500), [1.0, 1.0, 1.0]);
        let w = temperature_rgb(3400);
        assert!(w[0] > 0.99 && w[1] < 0.9 && w[2] < w[1], "{w:?}");
    }
}
