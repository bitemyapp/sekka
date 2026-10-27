//! The displays as the compositor reports them (`aizome-ipc`'s `outputs`),
//! the settings the pane changes, and the arithmetic behind it: refresh
//! rates, scales, rotations, and the arrangement (where a dropped display
//! lands, who follows a display that grows, what turning one off or
//! mirroring it does to the others).
//!
//! Ported from Tatami's Displays (`desktop/tatami/displays/src/model.rs`),
//! with Hyprland's monitor rules replaced by the compositor's
//! `output-config` requests and Aizome's `[[outputs]]` settings.

use aizome_config::OutputConfig;
use aizome_ipc::{OutputInfo, Request, Transform};

/// A display mode: its size in pixels and refresh rate in millihertz.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mode {
    pub width: i32,
    pub height: i32,
    /// Millihertz, as the compositor counts.
    pub refresh: i32,
}

impl Mode {
    pub fn new(width: i32, height: i32, refresh: i32) -> Mode {
        Mode { width, height, refresh }
    }

    /// The mode as the compositor and the settings file take it:
    /// `"2880x1800@120"` (`"2880x1800@59.94"` for fractional rates).
    pub fn setting(&self) -> String {
        if self.refresh <= 0 {
            return format!("{}x{}", self.width, self.height);
        }
        let hz = self.refresh as f64 / 1000.0;
        if (hz - hz.round()).abs() < 0.0005 {
            format!("{}x{}@{}", self.width, self.height, hz.round() as i64)
        } else {
            format!("{}x{}@{:.3}", self.width, self.height, hz)
        }
    }
}

/// "120 Hz", "59.94 Hz".
pub fn refresh_label(refresh_mhz: i32) -> String {
    let hz = refresh_mhz as f64 / 1000.0;
    if (hz - hz.round()).abs() < 0.01 { format!("{:.0} Hz", hz) } else { format!("{:.2} Hz", hz) }
}

/// "3840 × 2160".
pub fn size_label(width: i32, height: i32) -> String {
    format!("{width} \u{d7} {height}")
}

/// The eight rotations and reflections, in the order the pane lists them.
pub const ROTATIONS: [(Transform, &str); 8] = [
    (Transform::Normal, "Standard"),
    (Transform::_90, "90°"),
    (Transform::_180, "180°"),
    (Transform::_270, "270°"),
    (Transform::Flipped, "Flipped"),
    (Transform::Flipped90, "Flipped, 90°"),
    (Transform::Flipped180, "Flipped, 180°"),
    (Transform::Flipped270, "Flipped, 270°"),
];

/// Where a transform is in [`ROTATIONS`].
pub fn rotation_index(t: Transform) -> usize {
    ROTATIONS.iter().position(|(r, _)| *r == t).unwrap_or(0)
}

/// Whether a transform turns the picture on its side.
pub fn sideways(t: Transform) -> bool {
    matches!(t, Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270)
}

pub fn config_transform(t: Transform) -> aizome_config::Transform {
    use aizome_config::Transform as C;
    match t {
        Transform::Normal => C::Normal,
        Transform::_90 => C::_90,
        Transform::_180 => C::_180,
        Transform::_270 => C::_270,
        Transform::Flipped => C::Flipped,
        Transform::Flipped90 => C::Flipped90,
        Transform::Flipped180 => C::Flipped180,
        Transform::Flipped270 => C::Flipped270,
    }
}

pub fn ipc_transform(t: aizome_config::Transform) -> Transform {
    use aizome_config::Transform as C;
    match t {
        C::Normal => Transform::Normal,
        C::_90 => Transform::_90,
        C::_180 => Transform::_180,
        C::_270 => Transform::_270,
        C::Flipped => Transform::Flipped,
        C::Flipped90 => Transform::Flipped90,
        C::Flipped180 => Transform::Flipped180,
        C::Flipped270 => Transform::Flipped270,
    }
}

/// A rectangle in the arrangement, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

/// A display: who it is, what it can do, and how it's set now.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// The connector: `DP-1`, `eDP-1`, `HEADLESS-1`.
    pub name: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    /// Modes it offers, as reported (the preferred one marked).
    pub modes: Vec<Mode>,
    pub preferred: Option<Mode>,
    pub physical_mm: (i32, i32),
    /// Reported by the compositor now (else known only from the settings
    /// file, as a display that's off).
    pub present: bool,
    pub settings: Settings,
}

/// What the pane sets for a display.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub name: String,
    pub enabled: bool,
    pub mode: Mode,
    /// Top-left corner in the arrangement, in points.
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: Transform,
    /// Showing the display with this connector (placed over it).
    pub mirror: Option<String>,
}

impl Settings {
    pub fn logical_size(&self) -> (i32, i32) {
        logical_size(&self.mode, self.scale, self.transform)
    }
}

/// The size in the arrangement: the mode divided by the scale, turned with
/// the display.
pub fn logical_size(mode: &Mode, scale: f64, transform: Transform) -> (i32, i32) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let w = (mode.width as f64 / scale).round() as i32;
    let h = (mode.height as f64 / scale).round() as i32;
    if sideways(transform) { (h, w) } else { (w, h) }
}

impl Display {
    /// A display the compositor reports, with what the settings file adds
    /// that the compositor can't say (mirroring).
    pub fn from_output(o: &OutputInfo, entry: Option<&OutputConfig>) -> Display {
        let modes: Vec<Mode> = o.modes.iter().map(|m| Mode::new(m.width, m.height, m.refresh)).collect();
        let preferred = o.modes.iter().find(|m| m.preferred).map(|m| Mode::new(m.width, m.height, m.refresh));
        // A display without a mode (off since the session began) takes its
        // preferred one, for when it's turned on.
        let mode = o
            .current_mode
            .as_ref()
            .map(|m| Mode::new(m.width, m.height, m.refresh))
            .filter(|m| m.width > 0 && m.height > 0)
            .or(preferred)
            .or_else(|| modes.first().copied())
            .unwrap_or_default();
        let mut modes = modes;
        if mode.width > 0 && !modes.contains(&mode) {
            modes.push(mode);
        }
        // The compositor says what it mirrors (a mirror previewed, or set
        // elsewhere); the settings file, for a compositor that doesn't.
        let mirror = o.mirror.clone().filter(|m| !m.is_empty()).or_else(|| entry.and_then(mirror_of));
        Display {
            name: o.name.clone(),
            make: o.make.trim().to_owned(),
            model: o.model.trim().to_owned(),
            serial: o.serial.trim().to_owned(),
            modes,
            preferred,
            physical_mm: o.physical_size_mm,
            present: true,
            settings: Settings {
                name: o.name.clone(),
                enabled: o.enabled,
                mode,
                x: o.logical.x,
                y: o.logical.y,
                scale: if o.scale > 0.0 { o.scale } else { 1.0 },
                transform: o.transform,
                mirror,
            },
        }
    }

    pub fn is_builtin(&self) -> bool {
        ["eDP", "LVDS", "DSI"].iter().any(|p| self.name.starts_with(p))
    }

    /// The name shown: "Built-in Display", or the maker and model.
    pub fn title(&self) -> String {
        if self.is_builtin() {
            return "Built-in Display".to_owned();
        }
        let make = self.make.split_whitespace().next().unwrap_or("").trim_end_matches([',', '.']);
        let model = self.model.trim();
        match (
            make.is_empty() || make.eq_ignore_ascii_case("unknown"),
            model.is_empty() || model.eq_ignore_ascii_case("unknown"),
        ) {
            (false, false) => format!("{make} {model}"),
            (true, false) => model.to_owned(),
            (false, true) => make.to_owned(),
            (true, true) => self.name.clone(),
        }
    }

    pub fn diagonal_inches(&self) -> Option<f64> {
        let (w, h) = self.physical_mm;
        (w > 0 && h > 0).then(|| (w as f64).hypot(h as f64) / 25.4)
    }

    /// Refresh rates for a resolution, highest first, without
    /// near-duplicates.
    pub fn refresh_rates(&self, width: i32, height: i32) -> Vec<i32> {
        refresh_rates(&self.modes, width, height)
    }

    /// Resolutions, largest first.
    pub fn resolutions(&self) -> Vec<(i32, i32)> {
        let mut sizes: Vec<(i32, i32)> = Vec::new();
        for m in &self.modes {
            if !sizes.contains(&(m.width, m.height)) {
                sizes.push((m.width, m.height));
            }
        }
        sizes.sort_by_key(|(w, h)| std::cmp::Reverse(w * h));
        sizes
    }

    /// The key the settings file keeps it under: make, model and serial
    /// when it has them, else its connector.
    pub fn config_key(&self) -> ConfigKey {
        ConfigKey {
            name: self.name.clone(),
            make: self.make.clone(),
            model: self.model.clone(),
            serial: self.serial.clone(),
            by_connector: false,
        }
    }
}

/// How a display is found in `[[outputs]]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigKey {
    pub name: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    /// Another display has the same make, model and serial (two of one
    /// monitor, or virtual outputs): its connector tells them apart.
    pub by_connector: bool,
}

impl ConfigKey {
    /// Whether make and model say which display this is (else the
    /// connector does).
    pub fn identified(&self) -> bool {
        let known = |s: &str| !s.is_empty() && !s.eq_ignore_ascii_case("unknown");
        !self.by_connector && known(&self.make) && known(&self.model)
    }

    /// Whether this is the same display as `other` by make, model and
    /// serial.
    pub fn same_kind(&self, other: &ConfigKey) -> bool {
        self.make == other.make && self.model == other.model && self.serial == other.serial
    }

    /// A new `[[outputs]]` entry for it, without settings yet.
    pub fn entry(&self) -> OutputConfig {
        if self.identified() {
            OutputConfig {
                make: Some(self.make.clone()),
                model: Some(self.model.clone()),
                serial: Some(self.serial.clone()).filter(|s| !s.is_empty()),
                ..OutputConfig::default()
            }
        } else {
            OutputConfig { name: Some(self.name.clone()), ..OutputConfig::default() }
        }
    }

    /// Whether an entry applies to this display (as the compositor
    /// decides).
    pub fn matches(&self, entry: &OutputConfig) -> bool {
        entry.matches(&self.name, &self.make, &self.model, &self.serial)
    }

    /// Whether an entry is this display's own: the one the pane writes.
    pub fn owns(&self, entry: &OutputConfig) -> bool {
        if self.identified() {
            entry.name.is_none()
                && entry.make.as_deref() == Some(self.make.as_str())
                && entry.model.as_deref() == Some(self.model.as_str())
                && entry.serial.as_deref().unwrap_or("") == self.serial
        } else {
            entry.name.as_deref() == Some(self.name.as_str())
                && entry.make.is_none()
                && entry.model.is_none()
                && entry.serial.is_none()
        }
    }
}

/// The keys the displays are kept under: make, model and serial, or the
/// connector for displays that share them.
pub fn config_keys(displays: &[Display]) -> Vec<ConfigKey> {
    let keys: Vec<ConfigKey> = displays.iter().map(Display::config_key).collect();
    keys.iter()
        .enumerate()
        .map(|(i, k)| {
            let shared = keys.iter().enumerate().any(|(j, o)| j != i && o.same_kind(k));
            ConfigKey { by_connector: shared, ..k.clone() }
        })
        .collect()
}

/// The connector a settings entry says its display mirrors.
pub fn mirror_of(entry: &OutputConfig) -> Option<String> {
    entry.extra.get("mirror").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// Whether a settings entry marks its display the main one.
pub fn is_primary(entry: &OutputConfig) -> bool {
    entry.extra.get("primary").and_then(|v| v.as_bool()).unwrap_or(false)
}

pub fn refresh_rates(modes: &[Mode], width: i32, height: i32) -> Vec<i32> {
    let mut rates: Vec<i32> = Vec::new();
    for m in modes.iter().filter(|m| m.width == width && m.height == height) {
        if !rates.iter().any(|r| (r - m.refresh).abs() < 10) {
            rates.push(m.refresh);
        }
    }
    rates.sort_by(|a, b| b.cmp(a));
    rates
}

/// The listed rate nearest `refresh` (59997 mHz is listed as 60000).
pub fn nearest(rates: &[i32], refresh: i32) -> Option<usize> {
    (0..rates.len()).min_by_key(|i| (rates[*i] - refresh).abs())
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Scales from 1 to 4 that leave whole logical pixels: steps of 1/120
/// (fractional-scale-v1's) that divide both sides.
pub fn valid_scales(width: i32, height: i32) -> Vec<f64> {
    let whole = gcd(width as i64 * 120, height as i64 * 120).max(1);
    (120..=480).filter(|steps| whole % steps == 0).map(|steps| steps as f64 / 120.0).collect()
}

/// The nearest valid scale at or above `scale`.
pub fn clean_scale(scale: f64, width: i32, height: i32) -> f64 {
    let valid = valid_scales(width, height);
    valid.iter().copied().find(|v| *v >= scale - 1e-9).or_else(|| valid.last().copied()).unwrap_or(1.0)
}

/// The smallest arrangement a scale may leave: below this, windows don't
/// fit (macOS stops at about the same).
pub const MIN_LOGICAL: (i32, i32) = (800, 500);

fn roomy(width: i32, height: i32, scale: f64) -> bool {
    let (w, h) = logical_size(&Mode::new(width, height, 0), scale, Transform::Normal);
    w.max(h) >= MIN_LOGICAL.0 && w.min(h) >= MIN_LOGICAL.1
}

/// Whole scales (1×, 2×, 3×) that leave room to work, and the current
/// scale: what the pane offers first.
pub fn integer_scales(width: i32, height: i32, current: f64) -> Vec<f64> {
    let mut out: Vec<f64> = [1.0, 2.0, 3.0].into_iter().filter(|s| *s == 1.0 || roomy(width, height, *s)).collect();
    if is_integer(current) && !out.iter().any(|s| (s - current).abs() < 1e-6) {
        out.push(current);
    }
    out.sort_by(|a, b| a.total_cmp(b));
    out
}

/// Fractional scales, when asked for: the usual steps, moved to the
/// nearest that keeps whole pixels, and the current one.
pub fn fractional_scales(width: i32, height: i32, current: f64) -> Vec<f64> {
    let mut out: Vec<f64> = [1.25, 1.5, 1.75, 2.25, 2.5]
        .into_iter()
        .map(|s| clean_scale(s, width, height))
        .filter(|s| !is_integer(*s) && roomy(width, height, *s))
        .collect();
    if !is_integer(current) {
        out.push(current);
    }
    out.sort_by(|a, b| a.total_cmp(b));
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    out
}

pub fn is_integer(scale: f64) -> bool {
    (scale - scale.round()).abs() < 1e-6
}

/// "2×", "1.5×", "1.33×".
pub fn scale_label(scale: f64) -> String {
    if is_integer(scale) {
        format!("{}\u{d7}", scale.round() as i64)
    } else {
        let s = format!("{:.2}", scale);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        format!("{s}\u{d7}")
    }
}

/// Where a display dropped at `dropped` goes, as macOS places it: beside,
/// above or below another display, sharing an edge with it and overlapping
/// none, as close as that allows to where it was dropped. Within
/// `tolerance` of another display's edges or middle, it lines up with them.
pub fn snap(dropped: Rect, others: &[Rect], tolerance: i32) -> (i32, i32) {
    if others.is_empty() {
        return (0, 0);
    }
    // The nearest of the edges and the middle within the tolerance.
    let line_up = |value: i32, targets: [i32; 3]| {
        targets
            .into_iter()
            .filter(|t| (t - value).abs() <= tolerance)
            .min_by_key(|t| (t - value).abs())
            .unwrap_or(value)
    };
    let mut candidates = Vec::new();
    for other in others {
        // Beside it: some of the height shared.
        let (low, high) = (other.y - dropped.h + 1, other.y + other.h - 1);
        for x in [other.x - dropped.w, other.x + other.w] {
            let y = line_up(
                dropped.y.clamp(low, high),
                [other.y, other.y + other.h - dropped.h, other.y + (other.h - dropped.h) / 2],
            );
            candidates.push((x, y.clamp(low, high)));
        }
        // Above or below it: some of the width shared.
        let (low, high) = (other.x - dropped.w + 1, other.x + other.w - 1);
        for y in [other.y - dropped.h, other.y + other.h] {
            let x = line_up(
                dropped.x.clamp(low, high),
                [other.x, other.x + other.w - dropped.w, other.x + (other.w - dropped.w) / 2],
            );
            candidates.push((x.clamp(low, high), y));
        }
    }
    let distance = |(x, y): (i32, i32)| {
        let (dx, dy) = ((x - dropped.x) as i64, (y - dropped.y) as i64);
        dx * dx + dy * dy
    };
    candidates
        .into_iter()
        .filter(|(x, y)| {
            let placed = Rect { x: *x, y: *y, ..dropped };
            !others.iter().any(|o| placed.overlaps(o))
        })
        .min_by_key(|c| distance(*c))
        .unwrap_or((dropped.x, dropped.y))
}

/// Whether a display has a place of its own in the arrangement: on, and
/// not mirroring.
pub fn arranged(s: &Settings) -> bool {
    s.enabled && s.mirror.is_none()
}

pub fn rect(s: &Settings) -> Rect {
    let (w, h) = s.logical_size();
    Rect { x: s.x, y: s.y, w, h }
}

/// Start the arrangement at 0,0 (mirrors follow their source).
pub fn normalize(settings: &mut [Settings]) {
    let rects: Vec<Rect> = settings.iter().filter(|s| arranged(s)).map(rect).collect();
    let min_x = rects.iter().map(|r| r.x).min().unwrap_or(0);
    let min_y = rects.iter().map(|r| r.y).min().unwrap_or(0);
    for s in settings.iter_mut().filter(|s| arranged(s)) {
        s.x -= min_x;
        s.y -= min_y;
    }
    follow_sources(settings);
}

/// Mirrors sit where their source is.
pub fn follow_sources(settings: &mut [Settings]) {
    let sources: Vec<(String, i32, i32)> =
        settings.iter().filter(|s| arranged(s)).map(|s| (s.name.clone(), s.x, s.y)).collect();
    for s in settings.iter_mut() {
        if let Some(src) = &s.mirror
            && let Some((_, x, y)) = sources.iter().find(|(n, _, _)| n == src)
        {
            (s.x, s.y) = (*x, *y);
        }
    }
}

/// A display dropped at `x`,`y`: beside the others (snap), lining up
/// within a sixteenth of its size.
pub fn place(settings: &mut [Settings], name: &str, x: i32, y: i32) {
    let Some(i) = settings.iter().position(|s| s.name == name) else { return };
    let dropped = Rect { x, y, ..rect(&settings[i]) };
    let others: Vec<Rect> = settings.iter().filter(|s| s.name != name && arranged(s)).map(rect).collect();
    let tolerance = dropped.w.max(dropped.h) / 16;
    (settings[i].x, settings[i].y) = snap(dropped, &others, tolerance);
    normalize(settings);
}

/// A display changed size (scale, resolution or rotation), from `old`: the
/// displays to its right and below move with its edges, so they stay beside
/// it.
pub fn resized(settings: &mut [Settings], name: &str, old: (i32, i32)) {
    let Some(i) = settings.iter().position(|s| s.name == name) else { return };
    let (x, y) = (settings[i].x, settings[i].y);
    let new = settings[i].logical_size();
    for (j, s) in settings.iter_mut().enumerate() {
        if j == i || !arranged(s) {
            continue;
        }
        if s.x >= x + old.0 {
            s.x += new.0 - old.0;
        }
        if s.y >= y + old.1 {
            s.y += new.1 - old.1;
        }
    }
    normalize(settings);
}

/// What a display is used as ("Use as" in macOS).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Use {
    Main,
    Extended,
    /// Showing the display with this connector.
    Mirror(String),
    Off,
}

/// Turn a display on or off, extend the desktop onto it, make it the main
/// display or have it mirror another. The main display moves to another
/// display when it stops being one of the arrangement.
pub fn use_as(settings: &mut [Settings], main: &mut Option<String>, name: &str, used: Use) {
    let Some(i) = settings.iter().position(|s| s.name == name) else { return };
    let was_arranged = arranged(&settings[i]);
    let is_main = main.as_deref() == Some(name);
    match &used {
        Use::Main | Use::Extended => {
            settings[i].enabled = true;
            settings[i].mirror = None;
        }
        Use::Mirror(source) => {
            settings[i].enabled = true;
            settings[i].mirror = Some(source.clone());
            // As near the source's size as its pixels allow, so the same
            // picture fills both.
            if let Some(src) = settings.iter().find(|s| s.name == *source).cloned() {
                let (sw, _) = src.logical_size();
                let m = &settings[i];
                let across = if sideways(m.transform) { m.mode.height } else { m.mode.width };
                if sw > 0 {
                    let scale = (across as f64 / sw as f64).clamp(0.5, 4.0);
                    settings[i].scale = (scale * 120.0).round() / 120.0;
                }
            }
        }
        Use::Off => {
            settings[i].enabled = false;
            settings[i].mirror = None;
        }
    }
    match &used {
        Use::Main => *main = Some(name.to_owned()),
        Use::Mirror(source) if is_main => *main = Some(source.clone()),
        Use::Extended if is_main => {
            if let Some(other) = settings.iter().find(|s| s.name != name && arranged(s)) {
                *main = Some(other.name.clone());
            }
        }
        _ => {}
    }
    if !arranged(&settings[i]) {
        // Displays that mirrored it show their own picture again, beside
        // the others.
        let freed: Vec<String> =
            settings.iter().filter(|s| s.mirror.as_deref() == Some(name)).map(|s| s.name.clone()).collect();
        for f in freed {
            if let Some(j) = settings.iter().position(|s| s.name == f) {
                settings[j].mirror = None;
                put_beside(settings, j);
            }
        }
    } else if !was_arranged {
        put_beside(settings, i);
    }
    // The main display is one of the arrangement.
    let usable = |m: &str| settings.iter().any(|s| s.name == m && arranged(s));
    if !main.as_deref().is_some_and(usable)
        && let Some(first) = settings.iter().find(|s| arranged(s))
    {
        *main = Some(first.name.clone());
    }
    normalize(settings);
}

/// On again: to the right of the others.
fn put_beside(settings: &mut [Settings], i: usize) {
    let name = settings[i].name.clone();
    let others: Vec<Rect> = settings.iter().filter(|s| s.name != name && arranged(s)).map(rect).collect();
    let right = others.iter().map(|r| r.x + r.w).max().unwrap_or(0);
    let dropped = Rect { x: right, y: 0, ..rect(&settings[i]) };
    (settings[i].x, settings[i].y) = snap(dropped, &others, 0);
}

/// The main display: as kept, or else the one at the arrangement's top
/// left, or else the first one on.
pub fn main_display(settings: &[Settings], kept: Option<&str>) -> Option<String> {
    let usable = |n: &str| settings.iter().any(|s| s.name == n && arranged(s));
    kept.filter(|m| usable(m)).map(str::to_owned).or_else(|| {
        settings
            .iter()
            .filter(|s| arranged(s))
            .min_by_key(|s| (s.y.max(0) + s.x.max(0), s.x, s.y))
            .map(|s| s.name.clone())
    })
}

/// The displays whose settings differ.
pub fn changed(before: &[Settings], after: &[Settings]) -> Vec<String> {
    after.iter().filter(|s| !before.contains(s)).map(|s| s.name.clone()).collect()
}

/// Whether a change can leave a screen blank (and so asks to be kept):
/// resolution, refresh rate, rotation, mirroring, or turning a display
/// off or on.
pub fn risky(before: &[Settings], after: &[Settings]) -> bool {
    after.iter().any(|a| match before.iter().find(|b| b.name == a.name) {
        Some(b) => a.mode != b.mode || a.transform != b.transform || a.mirror != b.mirror || a.enabled != b.enabled,
        None => true,
    })
}

/// The compositor's requests that make a display so.
pub fn request(s: &Settings) -> Request {
    if !s.enabled {
        return Request::OutputConfig {
            name: s.name.clone(),
            mode: None,
            scale: None,
            position: None,
            transform: None,
            enabled: Some(false),
        };
    }
    Request::OutputConfig {
        name: s.name.clone(),
        mode: Some(s.mode.setting()),
        scale: Some(s.scale),
        position: Some([s.x, s.y]),
        transform: Some(s.transform),
        enabled: Some(true),
    }
}

/// The requests for the displays that changed, those turned off last (so
/// a screen is never left without a display that's on). Mirroring goes
/// through the compositor at once too (`mirror-output`), so it's seen
/// before it's kept: a display stops mirroring before it's placed, and
/// starts once it's set.
pub fn requests(before: &[Settings], after: &[Settings], names: &[String]) -> Vec<Request> {
    let mut on: Vec<Request> = Vec::new();
    let mut off: Vec<Request> = Vec::new();
    for s in after.iter().filter(|s| names.contains(&s.name)) {
        let was = before.iter().find(|b| b.name == s.name).and_then(|b| b.mirror.clone());
        let mirror = |source: Option<String>| Request::MirrorOutput { name: s.name.clone(), source };
        if !s.enabled {
            off.push(request(s));
        } else if was != s.mirror && s.mirror.is_none() {
            on.push(mirror(None));
            on.push(request(s));
        } else {
            on.push(request(s));
            if was != s.mirror {
                on.push(mirror(s.mirror.clone()));
            }
        }
    }
    on.extend(off);
    on
}

/// `[[outputs]]` written for these displays: each matching entry updated
/// (or a new one added), keeping keys this pane doesn't set.
pub fn write_entries(outputs: &mut Vec<OutputConfig>, displays: &[(ConfigKey, Settings)], main: Option<&str>) {
    for (key, s) in displays {
        let index = match outputs.iter().position(|e| key.owns(e)) {
            Some(i) => i,
            // By connector: ahead of entries by make and model, which would
            // otherwise match it first.
            None if !key.identified() => {
                outputs.insert(0, key.entry());
                0
            }
            None => {
                outputs.push(key.entry());
                outputs.len() - 1
            }
        };
        let e = &mut outputs[index];
        e.enabled = Some(s.enabled);
        e.mode = Some(s.mode.setting());
        e.scale = Some(s.scale);
        e.position = Some([s.x, s.y]);
        e.transform = Some(config_transform(s.transform));
        match &s.mirror {
            Some(m) => {
                e.extra.insert("mirror".into(), toml::Value::String(m.clone()));
            }
            None => {
                e.extra.remove("mirror");
            }
        }
        if main == Some(s.name.as_str()) {
            e.extra.insert("primary".into(), toml::Value::Boolean(true));
        } else {
            e.extra.remove("primary");
        }
    }
    // One main display: the others' marks go.
    if let Some(main) = main {
        let main_key = displays.iter().find(|(_, s)| s.name == main).map(|(k, _)| k.clone());
        for e in outputs.iter_mut() {
            if main_key.as_ref().is_some_and(|k| k.owns(e)) {
                continue;
            }
            if is_primary(e) && main_key.is_some() {
                e.extra.remove("primary");
            }
        }
    }
}

/// Displays known only from the settings file (off, so the compositor
/// doesn't list them), to turn on again: by connector.
pub fn known_off(outputs: &[OutputConfig], present: &[Display]) -> Vec<Display> {
    outputs
        .iter()
        .filter(|e| e.enabled == Some(false))
        .filter_map(|e| {
            let name = e.name.clone()?;
            if present.iter().any(|d| d.name == name) {
                return None;
            }
            let (w, h, r) = e.parsed_mode()?;
            let mode = Mode::new(w, h, r.unwrap_or(60_000));
            Some(Display {
                name: name.clone(),
                make: e.make.clone().unwrap_or_default(),
                model: e.model.clone().unwrap_or_default(),
                serial: e.serial.clone().unwrap_or_default(),
                modes: vec![mode],
                preferred: Some(mode),
                physical_mm: (0, 0),
                present: false,
                settings: Settings {
                    name,
                    enabled: false,
                    mode,
                    x: e.position.map_or(0, |p| p[0]),
                    y: e.position.map_or(0, |p| p[1]),
                    scale: e.scale.unwrap_or(1.0),
                    transform: e.transform.map(ipc_transform).unwrap_or_default(),
                    mirror: None,
                },
            })
        })
        .collect()
}

/// Two displays at the same place and size show the same picture: the
/// later one counts as mirroring the earlier, when the settings file
/// didn't say.
pub fn infer_mirrors(displays: &mut [Display]) {
    let rects: Vec<(String, Rect, bool)> = displays
        .iter()
        .map(|d| (d.name.clone(), rect(&d.settings), d.settings.enabled && d.settings.mirror.is_none()))
        .collect();
    for (i, d) in displays.iter_mut().enumerate() {
        if d.settings.mirror.is_some() || !d.settings.enabled {
            continue;
        }
        let me = rects[i].1;
        if let Some((src, _, _)) =
            rects[..i].iter().find(|(_, r, on)| *on && r.x == me.x && r.y == me.y && r.w == me.w && r.h == me.h)
        {
            d.settings.mirror = Some(src.clone());
        }
    }
    // A mirror whose source isn't on shows its own picture.
    let on: Vec<String> = displays.iter().filter(|d| arranged(&d.settings)).map(|d| d.name.clone()).collect();
    for d in displays.iter_mut() {
        if d.settings.mirror.as_ref().is_some_and(|m| !on.contains(m)) {
            d.settings.mirror = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_ipc::Mode as IpcMode;

    fn output(name: &str, w: i32, h: i32, scale: f64, x: i32) -> OutputInfo {
        let mode = IpcMode { width: w, height: h, refresh: 60_000, preferred: true };
        let (lw, lh) = ((w as f64 / scale) as i32, (h as f64 / scale) as i32);
        OutputInfo {
            name: name.into(),
            make: "Samsung Display Corp.".into(),
            model: "ATNA40HQ10-0".into(),
            serial: "0x3F".into(),
            enabled: true,
            current_mode: Some(mode.clone()),
            modes: vec![
                mode,
                IpcMode { width: w, height: h, refresh: 120_000, preferred: false },
                IpcMode { width: 1920, height: 1080, refresh: 59_940, preferred: false },
            ],
            scale,
            transform: Transform::Normal,
            logical: aizome_ipc::Rect::new(x, 0, lw, lh),
            usable: aizome_ipc::Rect::new(x, 0, lw, lh),
            physical_size_mm: (300, 190),
            powered: true,
            ..OutputInfo::default()
        }
    }

    #[test]
    fn displays_are_read_from_the_compositor() {
        let d = Display::from_output(&output("eDP-1", 2880, 1800, 2.0, 0), None);
        assert_eq!(d.title(), "Built-in Display");
        assert_eq!(d.settings.logical_size(), (1440, 900));
        assert_eq!(d.refresh_rates(2880, 1800), [120_000, 60_000]);
        assert_eq!(d.resolutions(), [(2880, 1800), (1920, 1080)]);
        assert_eq!(format!("{:.0}", d.diagonal_inches().unwrap()), "14");
        let ext = Display::from_output(&output("DP-1", 3840, 2160, 2.0, 1440), None);
        assert_eq!(ext.title(), "Samsung ATNA40HQ10-0");
        assert_eq!(refresh_label(59_940), "59.94 Hz");
        assert_eq!(refresh_label(120_000), "120 Hz");
        assert_eq!(Mode::new(2880, 1800, 120_000).setting(), "2880x1800@120");
        assert_eq!(Mode::new(1920, 1080, 59_940).setting(), "1920x1080@59.940");
        assert_eq!(aizome_config::parse_mode("1920x1080@59.940"), Some((1920, 1080, Some(59_940))));
    }

    #[test]
    fn scales_are_whole_first_and_fractional_when_asked() {
        assert_eq!(integer_scales(2880, 1800, 2.0), [1.0, 2.0, 3.0]);
        // 1366 × 768 at 2× would leave 683 × 384: too small.
        assert_eq!(integer_scales(1366, 768, 1.0), [1.0]);
        let f = fractional_scales(2880, 1800, 2.0);
        assert!(f.contains(&1.25) && f.contains(&1.5));
        for s in &f {
            assert_eq!((2880.0 / s).fract(), 0.0);
            assert_eq!((1800.0 / s).fract(), 0.0);
        }
        assert_eq!(scale_label(2.0), "2\u{d7}");
        assert_eq!(scale_label(1.5), "1.5\u{d7}");
        assert_eq!(scale_label(1.25), "1.25\u{d7}");
    }

    fn two() -> (Vec<Settings>, Option<String>) {
        let a = Display::from_output(&output("DP-1", 3840, 2160, 2.0, 0), None).settings;
        let b = Display::from_output(&output("eDP-1", 2880, 1800, 2.0, 1920), None).settings;
        (vec![a, b], Some("DP-1".into()))
    }

    #[test]
    fn dropped_displays_share_an_edge_and_never_overlap() {
        let main = Rect::new(0, 0, 1920, 1080);
        let laptop = Rect::new(0, 0, 1440, 900);
        assert_eq!(snap(Rect { x: 1500, y: 30, ..laptop }, &[main], 60), (1920, 0));
        assert_eq!(snap(Rect { x: -1600, y: 200, ..laptop }, &[main], 60), (-1440, 180));
        assert_eq!(snap(Rect { x: 230, y: -1200, ..laptop }, &[main], 60), (240, -900));
        assert_eq!(snap(Rect { x: 9000, y: 5000, ..laptop }, &[main], 60), (1920, 1079));
        assert_eq!(snap(laptop, &[], 60), (0, 0));
        let (mut s, _) = two();
        place(&mut s, "eDP-1", -1500, 100);
        assert_eq!((s[1].x, s[1].y), (0, 90));
        assert_eq!((s[0].x, s[0].y), (1440, 0));
        assert!(!rect(&s[0]).overlaps(&rect(&s[1])));
    }

    #[test]
    fn neighbours_follow_a_display_that_changes_size() {
        let (mut s, _) = two();
        let old = s[0].logical_size();
        s[0].scale = 1.0;
        resized(&mut s, "DP-1", old);
        assert_eq!((s[1].x, s[1].y), (3840, 0));
    }

    #[test]
    fn the_main_display_moves_when_it_stops_being_one() {
        let (mut s, mut main) = two();
        use_as(&mut s, &mut main, "DP-1", Use::Off);
        assert!(!s[0].enabled);
        assert_eq!(main.as_deref(), Some("eDP-1"));
        assert_eq!((s[1].x, s[1].y), (0, 0));

        let (mut s, mut main) = two();
        use_as(&mut s, &mut main, "DP-1", Use::Mirror("eDP-1".into()));
        assert_eq!(s[0].mirror.as_deref(), Some("eDP-1"));
        assert_eq!(main.as_deref(), Some("eDP-1"));
        // Placed over its source, as wide (16:9 over 16:10: the same
        // width, a little less height).
        assert_eq!((s[0].x, s[0].y), (s[1].x, s[1].y));
        assert_eq!(s[0].logical_size(), (1440, 810));
        assert_eq!(s[1].logical_size(), (1440, 900));

        let (mut s, mut main) = two();
        use_as(&mut s, &mut main, "eDP-1", Use::Main);
        assert_eq!(main.as_deref(), Some("eDP-1"));
        use_as(&mut s, &mut main, "eDP-1", Use::Extended);
        assert_eq!(main.as_deref(), Some("DP-1"));
    }

    #[test]
    fn a_display_turned_on_goes_beside_the_others() {
        let (mut s, mut main) = two();
        s[1].enabled = false;
        (s[1].x, s[1].y) = (0, 0);
        use_as(&mut s, &mut main, "eDP-1", Use::Extended);
        assert!(s[1].enabled);
        assert!(!rect(&s[0]).overlaps(&rect(&s[1])));
        assert_eq!((s[1].x, s[1].y), (1920, 0));
        // A display that mirrored one turned off shows its own picture.
        let (mut s, mut main) = two();
        use_as(&mut s, &mut main, "eDP-1", Use::Mirror("DP-1".into()));
        use_as(&mut s, &mut main, "DP-1", Use::Off);
        assert!(s[1].mirror.is_none() && s[1].enabled);
        assert_eq!(main.as_deref(), Some("eDP-1"));
    }

    #[test]
    fn changes_become_requests_and_entries() {
        let (before, main) = two();
        let mut after = before.clone();
        after[1].mode = Mode::new(2880, 1800, 120_000);
        let names = changed(&before, &after);
        assert_eq!(names, ["eDP-1"]);
        assert!(risky(&before, &after));
        let mut scaled = before.clone();
        scaled[1].scale = 1.5;
        assert!(!risky(&before, &scaled));
        let asked = requests(&before, &after, &names);
        assert_eq!(asked.len(), 1, "no mirroring asked: {asked:?}");
        match &asked[0] {
            Request::OutputConfig { name, mode, scale, position, .. } => {
                assert_eq!(name, "eDP-1");
                assert_eq!(mode.as_deref(), Some("2880x1800@120"));
                assert_eq!(*scale, Some(2.0));
                assert_eq!(*position, Some([1920, 0]));
            }
            other => panic!("{other:?}"),
        }
        // Mirroring is asked of the compositor at once: after the display
        // is set when it starts, before it's placed when it stops.
        let (mut mirrored, mut main2) = two();
        use_as(&mut mirrored, &mut main2, "eDP-1", Use::Mirror("DP-1".into()));
        let names = changed(&before, &mirrored);
        let asked = requests(&before, &mirrored, &names);
        assert!(matches!(asked.first(), Some(Request::OutputConfig { name, .. }) if name == "eDP-1"), "{asked:?}");
        assert!(
            matches!(asked.last(), Some(Request::MirrorOutput { name, source: Some(s) }) if name == "eDP-1" && s == "DP-1"),
            "{asked:?}"
        );
        let back = requests(&mirrored, &before, &changed(&mirrored, &before));
        assert!(
            matches!(back.first(), Some(Request::MirrorOutput { name, source: None }) if name == "eDP-1"),
            "{back:?}"
        );
        assert!(matches!(back.last(), Some(Request::OutputConfig { .. })), "{back:?}");
        let edp = Display::from_output(&output("eDP-1", 2880, 1800, 2.0, 1920), None);
        let mut outputs = vec![OutputConfig { name: Some("HDMI-A-1".into()), ..Default::default() }];
        outputs[0].extra.insert("primary".into(), toml::Value::Boolean(true));
        write_entries(&mut outputs, &[(edp.config_key(), after[1].clone())], Some("eDP-1"));
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[1].make.as_deref(), Some("Samsung Display Corp."));
        assert_eq!(outputs[1].mode.as_deref(), Some("2880x1800@120"));
        assert!(is_primary(&outputs[1]) && !is_primary(&outputs[0]));
        let _ = main;
    }

    #[test]
    fn identical_displays_are_kept_apart_by_connector() {
        let mut a = Display::from_output(&output("HEADLESS-1", 1920, 1080, 1.0, 0), None);
        let mut b = Display::from_output(&output("HEADLESS-2", 2880, 1800, 2.0, 1920), None);
        a.serial.clear();
        b.serial.clear();
        let keys = config_keys(&[a.clone(), b.clone()]);
        assert!(keys.iter().all(|k| k.by_connector && !k.identified()));
        // An old entry by make and model is shadowed by theirs.
        let mut outputs =
            vec![OutputConfig { make: Some(a.make.clone()), model: Some(a.model.clone()), ..Default::default() }];
        write_entries(
            &mut outputs,
            &[(keys[0].clone(), a.settings.clone()), (keys[1].clone(), b.settings.clone())],
            Some("HEADLESS-1"),
        );
        assert_eq!(outputs.len(), 3);
        let first = |name: &str| outputs.iter().find(|e| e.matches(name, &a.make, &a.model, "")).unwrap().clone();
        assert_eq!(first("HEADLESS-1").mode.as_deref(), Some("1920x1080@60"));
        assert_eq!(first("HEADLESS-2").mode.as_deref(), Some("2880x1800@60"));
    }

    #[test]
    fn displays_at_the_same_place_mirror() {
        let mut ds = vec![
            Display::from_output(&output("eDP-1", 2880, 1800, 2.0, 0), None),
            Display::from_output(&output("DP-1", 2880, 1800, 2.0, 0), None),
        ];
        infer_mirrors(&mut ds);
        assert_eq!(ds[1].settings.mirror.as_deref(), Some("eDP-1"));
        assert_eq!(ds[0].settings.mirror, None);
    }
}
