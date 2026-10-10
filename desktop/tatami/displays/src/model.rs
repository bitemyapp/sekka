// SPDX-License-Identifier: MIT OR Apache-2.0
//! The displays as Hyprland reports them (`hyprctl -j monitors all`), the
//! settings the window sends to `tatami displays apply`, and the arithmetic
//! behind the window: refresh rates, scales and the arrangement.
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub width: i32,
    pub height: i32,
    pub refresh: f64,
}

impl Mode {
    /// A mode as hyprctl lists it: "3840x2160@60.00Hz".
    pub fn parse(text: &str) -> Option<Self> {
        let (size, refresh) = text.trim().trim_end_matches("Hz").split_once('@')?;
        let (width, height) = size.split_once('x')?;
        Some(Self {
            width: width.parse().ok()?,
            height: height.parse().ok()?,
            refresh: refresh.parse().ok()?,
        })
    }

    /// The mode in a monitor rule.
    pub fn setting(&self) -> String {
        format!("{}x{}@{:.3}", self.width, self.height, self.refresh)
    }

    pub fn same_size(&self, other: &Mode) -> bool {
        self.width == other.width && self.height == other.height
    }
}

/// "240 Hz", "119.88 Hz".
pub fn refresh_label(refresh: f64) -> String {
    if (refresh - refresh.round()).abs() < 0.01 {
        format!("{:.0} Hz", refresh)
    } else {
        format!("{:.2} Hz", refresh)
    }
}

/// "3840 × 2160".
pub fn size_label(width: i32, height: i32) -> String {
    format!("{width} \u{d7} {height}")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// The connector, such as DP-1.
    pub name: String,
    pub description: String,
    pub make: String,
    pub model: String,
    pub physical_mm: (f64, f64),
    pub modes: Vec<Mode>,
    pub enabled: bool,
    pub mode: Mode,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: i32,
    pub mirror_of: Option<String>,
    pub vrr_active: bool,
    pub cm: String,
    pub ten_bit: bool,
    pub sdr_brightness: f64,
    pub sdr_saturation: f64,
    pub workspace: i64,
}

impl Display {
    pub fn from_json(value: &Value) -> Option<Self> {
        let text = |key: &str| value[key].as_str().unwrap_or("").trim().to_owned();
        let number = |key: &str| value[key].as_f64();
        let mut modes: Vec<Mode> = value["availableModes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|mode| Mode::parse(mode.as_str()?))
            .collect();
        let reported = Mode {
            width: value["width"].as_i64()? as i32,
            height: value["height"].as_i64()? as i32,
            refresh: number("refreshRate")?,
        };
        // A display off since the session began reports no mode (0×0): its
        // first listed one, the preferred, for when it is turned on.
        let mode = if reported.width > 0 && reported.height > 0 {
            reported
        } else {
            modes.first().copied().unwrap_or(reported)
        };
        if mode.width > 0 && !modes.iter().any(|known| known.same_size(&mode)) {
            modes.push(mode);
        }
        let mirror = text("mirrorOf");
        Some(Self {
            name: value["name"].as_str()?.to_owned(),
            description: text("description"),
            make: text("make"),
            model: text("model"),
            physical_mm: (
                number("physicalWidth").unwrap_or(0.0),
                number("physicalHeight").unwrap_or(0.0),
            ),
            modes,
            enabled: value["disabled"] != true,
            mode,
            x: value["x"].as_i64().unwrap_or(0) as i32,
            y: value["y"].as_i64().unwrap_or(0) as i32,
            scale: number("scale").filter(|scale| *scale > 0.0).unwrap_or(1.0),
            transform: value["transform"].as_i64().unwrap_or(0) as i32,
            mirror_of: (!mirror.is_empty() && mirror != "none").then_some(mirror),
            vrr_active: value["vrr"] == true,
            cm: Some(text("colorManagementPreset"))
                .filter(|cm| !cm.is_empty())
                .unwrap_or_else(|| "srgb".to_owned()),
            ten_bit: text("currentFormat").contains("2101010"),
            sdr_brightness: number("sdrBrightness").unwrap_or(1.0),
            sdr_saturation: number("sdrSaturation").unwrap_or(1.0),
            workspace: value["activeWorkspace"]["id"].as_i64().unwrap_or(0),
        })
    }

    pub fn is_builtin(&self) -> bool {
        ["eDP", "LVDS", "DSI"]
            .iter()
            .any(|prefix| self.name.starts_with(prefix))
    }

    /// The name shown: "Built-in Display", or the maker and model.
    pub fn title(&self) -> String {
        if self.is_builtin() {
            return "Built-in Display".to_owned();
        }
        let make = self
            .make
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches([',', '.']);
        match (make.is_empty(), self.model.is_empty()) {
            (false, false) => format!("{make} {}", self.model),
            (true, false) => self.model.clone(),
            (false, true) => make.to_owned(),
            (true, true) => self.name.clone(),
        }
    }

    pub fn diagonal_inches(&self) -> Option<f64> {
        let (width, height) = self.physical_mm;
        (width > 0.0 && height > 0.0).then(|| width.hypot(height) / 25.4)
    }

    /// The size in the arrangement: the mode divided by the scale, turned
    /// with the display.
    pub fn logical_size(&self) -> (i32, i32) {
        logical_size(&self.mode, self.scale, self.transform)
    }

    /// Refresh rates for the current resolution, highest first, without the
    /// near-duplicates hyprctl lists.
    pub fn refresh_rates(&self) -> Vec<f64> {
        refresh_rates(&self.modes, self.mode.width, self.mode.height)
    }

    /// Resolutions, largest first.
    pub fn resolutions(&self) -> Vec<(i32, i32)> {
        let mut sizes: Vec<(i32, i32)> = vec![];
        for mode in &self.modes {
            if !sizes.contains(&(mode.width, mode.height)) {
                sizes.push((mode.width, mode.height));
            }
        }
        sizes.sort_by_key(|(width, height)| std::cmp::Reverse(width * height));
        sizes
    }
}

pub fn logical_size(mode: &Mode, scale: f64, transform: i32) -> (i32, i32) {
    let width = (mode.width as f64 / scale).round() as i32;
    let height = (mode.height as f64 / scale).round() as i32;
    if transform % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    }
}

pub fn refresh_rates(modes: &[Mode], width: i32, height: i32) -> Vec<f64> {
    let mut rates: Vec<f64> = vec![];
    for mode in modes
        .iter()
        .filter(|mode| mode.width == width && mode.height == height)
    {
        if !rates.iter().any(|rate| (rate - mode.refresh).abs() < 0.01) {
            rates.push(mode.refresh);
        }
    }
    rates.sort_by(|a, b| b.total_cmp(a));
    rates
}

/// The listed rate nearest `refresh`: the current rate, 59.997, is listed
/// as 60.00.
pub fn nearest(rates: &[f64], refresh: f64) -> Option<usize> {
    (0..rates.len()).min_by(|a, b| {
        (rates[*a] - refresh)
            .abs()
            .total_cmp(&(rates[*b] - refresh).abs())
    })
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Scales from 1 to 4 that Hyprland takes for a resolution without
/// fractional logical pixels: steps of 1/120 that divide both sides.
pub fn valid_scales(width: i32, height: i32) -> Vec<f64> {
    let whole = gcd(width as i64 * 120, height as i64 * 120).max(1);
    (120..=480)
        .filter(|steps| whole % steps == 0)
        .map(|steps| steps as f64 / 120.0)
        .collect()
}

/// The nearest valid scale at or above `scale` (as the helper's Super+/).
pub fn clean_scale(scale: f64, width: i32, height: i32) -> f64 {
    let valid = valid_scales(width, height);
    valid
        .iter()
        .copied()
        .find(|valid| *valid >= scale - 1e-9)
        .or_else(|| valid.last().copied())
        .unwrap_or(1.0)
}

/// The scaled resolutions offered first, from larger text to more space:
/// Omarchy's scale presets for this resolution, and the current scale.
pub fn scale_choices(width: i32, height: i32, current: f64) -> Vec<f64> {
    let mut choices: Vec<f64> = [3.0, 2.0, 1.6, 1.25, 1.0]
        .into_iter()
        .map(|preset| clean_scale(preset, width, height))
        .chain(std::iter::once(current))
        .collect();
    choices.sort_by(|a, b| b.total_cmp(a));
    choices.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    choices
}

/// "200%".
pub fn scale_label(scale: f64) -> String {
    let percent = scale * 100.0;
    if (percent - percent.round()).abs() < 0.05 {
        format!("{:.0}%", percent)
    } else {
        format!("{:.1}%", percent)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

/// Where a display dropped at `dropped` goes, as macOS places it: beside or
/// above or below another display, sharing an edge with it and overlapping
/// none, as close as that allows to where it was dropped. Within
/// `tolerance` of another display's edges or middle, it lines up with them.
pub fn snap(dropped: Rect, others: &[Rect], tolerance: i32) -> (i32, i32) {
    if others.is_empty() {
        return (0, 0);
    }
    let line_up = |value: i32, targets: [i32; 3]| {
        targets
            .into_iter()
            .find(|target| (target - value).abs() <= tolerance)
            .unwrap_or(value)
    };
    let mut candidates = vec![];
    for other in others {
        // Beside it: some of the height shared.
        let (low, high) = (other.y - dropped.h + 1, other.y + other.h - 1);
        for x in [other.x - dropped.w, other.x + other.w] {
            let y = line_up(
                dropped.y.clamp(low, high),
                [
                    other.y,
                    other.y + other.h - dropped.h,
                    other.y + (other.h - dropped.h) / 2,
                ],
            );
            candidates.push((x, y.clamp(low, high)));
        }
        // Above or below it: some of the width shared.
        let (low, high) = (other.x - dropped.w + 1, other.x + other.w - 1);
        for y in [other.y - dropped.h, other.y + other.h] {
            let x = line_up(
                dropped.x.clamp(low, high),
                [
                    other.x,
                    other.x + other.w - dropped.w,
                    other.x + (other.w - dropped.w) / 2,
                ],
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
            let placed = Rect {
                x: *x,
                y: *y,
                ..dropped
            };
            !others.iter().any(|other| placed.overlaps(other))
        })
        .min_by_key(|candidate| distance(*candidate))
        .unwrap_or((dropped.x, dropped.y))
}

/// The arrangement moved so it starts at 0,0.
pub fn normalized(rects: &[(String, Rect)]) -> Vec<(String, i32, i32)> {
    let min_x = rects.iter().map(|(_, rect)| rect.x).min().unwrap_or(0);
    let min_y = rects.iter().map(|(_, rect)| rect.y).min().unwrap_or(0);
    rects
        .iter()
        .map(|(name, rect)| (name.clone(), rect.x - min_x, rect.y - min_y))
        .collect()
}

/// What the window sends for a display: a monitor rule, and whether the
/// display is on (tool/src/displays.rs in the helper).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub mode: Mode,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: i32,
    pub mirror: Option<String>,
    pub vrr: i64,
    pub bitdepth: i64,
    pub cm: String,
    pub sdr_brightness: f64,
    pub sdr_saturation: f64,
}

impl Settings {
    /// A display as it is, with its variable refresh rate as kept (Hyprland
    /// reports only whether it is active now).
    pub fn of(display: &Display, vrr: i64) -> Self {
        Self {
            name: display.name.clone(),
            description: display.description.clone(),
            enabled: display.enabled,
            mode: display.mode,
            x: display.x,
            y: display.y,
            scale: display.scale,
            transform: display.transform,
            mirror: display.mirror_of.clone(),
            vrr,
            bitdepth: if display.ten_bit { 10 } else { 8 },
            cm: display.cm.clone(),
            sdr_brightness: display.sdr_brightness,
            sdr_saturation: display.sdr_saturation,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "name": self.name,
            "description": self.description,
            "enabled": self.enabled,
            "mode": self.mode.setting(),
            "position": format!("{}x{}", self.x, self.y),
            "scale": self.scale,
            "transform": self.transform,
            "vrr": self.vrr,
            "bitdepth": self.bitdepth,
            "cm": self.cm,
            "sdrbrightness": self.sdr_brightness,
            "sdrsaturation": self.sdr_saturation,
        });
        if let Some(mirror) = &self.mirror {
            value["mirror"] = json!(mirror);
        }
        value
    }

    pub fn logical_size(&self) -> (i32, i32) {
        logical_size(&self.mode, self.scale, self.transform)
    }

    pub fn is_hdr(&self) -> bool {
        self.cm == "hdr" || self.cm == "hdredid"
    }
}

/// Whether a display has a place in the arrangement: on, and not mirroring.
pub fn arranged(settings: &Settings) -> bool {
    settings.enabled && settings.mirror.is_none()
}

pub fn rect(settings: &Settings) -> Rect {
    let (w, h) = settings.logical_size();
    Rect {
        x: settings.x,
        y: settings.y,
        w,
        h,
    }
}

/// Start the arrangement at 0,0.
pub fn normalize(settings: &mut [Settings]) {
    let rects: Vec<(String, Rect)> = settings
        .iter()
        .filter(|s| arranged(s))
        .map(|s| (s.name.clone(), rect(s)))
        .collect();
    for (name, x, y) in normalized(&rects) {
        if let Some(s) = settings.iter_mut().find(|s| s.name == name) {
            (s.x, s.y) = (x, y);
        }
    }
}

/// A display dropped at `x`,`y` in the arrangement: beside the others
/// (snap), lining up within a sixteenth of its size.
pub fn place(settings: &mut [Settings], name: &str, x: i32, y: i32) {
    let Some(index) = settings.iter().position(|s| s.name == name) else {
        return;
    };
    let dropped = Rect {
        x,
        y,
        ..rect(&settings[index])
    };
    let others: Vec<Rect> = settings
        .iter()
        .filter(|s| s.name != name && arranged(s))
        .map(rect)
        .collect();
    let tolerance = dropped.w.max(dropped.h) / 16;
    (settings[index].x, settings[index].y) = snap(dropped, &others, tolerance);
    normalize(settings);
}

/// A display changed size (scale, resolution or rotation), from `old`: the
/// displays to its right and below move with its edges, so they stay beside
/// it.
pub fn resized(settings: &mut [Settings], name: &str, old: (i32, i32)) {
    let Some(index) = settings.iter().position(|s| s.name == name) else {
        return;
    };
    let (x, y) = (settings[index].x, settings[index].y);
    let new = settings[index].logical_size();
    for (other, s) in settings.iter_mut().enumerate() {
        if other == index || !arranged(s) {
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
#[derive(Clone, Debug, PartialEq)]
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
    let Some(index) = settings.iter().position(|s| s.name == name) else {
        return;
    };
    let was_arranged = arranged(&settings[index]);
    let is_main = main.as_deref() == Some(name);
    {
        let s = &mut settings[index];
        match &used {
            Use::Main | Use::Extended => {
                s.enabled = true;
                s.mirror = None;
            }
            Use::Mirror(source) => {
                s.enabled = true;
                s.mirror = Some(source.clone());
            }
            Use::Off => {
                s.enabled = false;
                s.mirror = None;
            }
        }
    }
    match &used {
        Use::Main => *main = Some(name.to_owned()),
        Use::Mirror(source) if is_main => *main = Some(source.clone()),
        // Another display becomes the main one, if there is another.
        Use::Extended if is_main => {
            if let Some(other) = settings.iter().find(|s| s.name != name && arranged(s)) {
                *main = Some(other.name.clone());
            }
        }
        _ => {}
    }
    if !arranged(&settings[index]) {
        // Displays that mirrored it show their own picture again.
        for s in settings.iter_mut() {
            if s.mirror.as_deref() == Some(name) {
                s.mirror = None;
            }
        }
    } else if !was_arranged {
        // On again: to the right of the others.
        let others: Vec<Rect> = settings
            .iter()
            .filter(|s| s.name != name && arranged(s))
            .map(rect)
            .collect();
        let right = others.iter().map(|r| r.x + r.w).max().unwrap_or(0);
        let dropped = Rect {
            x: right,
            y: 0,
            ..rect(&settings[index])
        };
        (settings[index].x, settings[index].y) = snap(dropped, &others, 0);
    }
    // The main display is one of the arrangement.
    let usable = |main: &str| settings.iter().any(|s| s.name == main && arranged(s));
    if !main.as_deref().is_some_and(usable)
        && let Some(first) = settings.iter().find(|s| arranged(s))
    {
        *main = Some(first.name.clone());
    }
    normalize(settings);
}

/// The key the helper keeps a display's settings under: its description,
/// or its connector when another display has the same one.
pub fn selector(display: &Display, all: &[Display]) -> String {
    let shared = all
        .iter()
        .filter(|other| other.description == display.description)
        .count()
        > 1;
    if display.description.is_empty() || shared {
        display.name.clone()
    } else {
        format!("desc:{}", display.description)
    }
}

/// The variable refresh rate kept for a display, if any.
pub fn kept_vrr(kept: &Value, display: &Display, all: &[Display]) -> Option<i64> {
    kept["displays"][selector(display, all)]["vrr"].as_i64()
}

/// The main display: where the cursor starts and workspace 1 opens. As
/// kept, or else the one showing workspace 1, or else the first one on.
pub fn main_display(displays: &[Display], kept: &Value) -> Option<String> {
    let usable = |name: &str| {
        displays
            .iter()
            .any(|display| display.name == name && display.enabled && display.mirror_of.is_none())
    };
    kept["main"]
        .as_str()
        .filter(|main| usable(main))
        .map(str::to_owned)
        .or_else(|| {
            displays
                .iter()
                .find(|display| display.workspace == 1 && usable(&display.name))
                .map(|display| display.name.clone())
        })
        .or_else(|| {
            displays
                .iter()
                .find(|display| usable(&display.name))
                .map(|display| display.name.clone())
        })
}

/// The displays whose settings differ.
pub fn changed(before: &[Settings], after: &[Settings]) -> Vec<String> {
    after
        .iter()
        .filter(|settings| !before.contains(settings))
        .map(|settings| settings.name.clone())
        .collect()
}

/// The JSON for `tatami displays apply`.
pub fn change_json(settings: &[Settings], main: Option<&str>, apply: &[String]) -> Value {
    json!({
        "displays": settings.iter().map(Settings::to_json).collect::<Vec<_>>(),
        "main": main,
        "apply": apply,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// From `hyprctl -j monitors all` on a laptop with a 4K 240 Hz display.
    fn monitors() -> Vec<Display> {
        let json = json!([
            {"name": "eDP-1", "description": "Samsung Display Corp. ATNA40HQ10-0  0x0000003F",
             "make": "Samsung Display Corp.", "model": "ATNA40HQ10-0 ", "width": 2880, "height": 1800,
             "physicalWidth": 300, "physicalHeight": 190, "refreshRate": 60.0, "x": 1920, "y": 0,
             "activeWorkspace": {"id": 0}, "scale": 2, "transform": 0, "vrr": false, "disabled": true,
             "currentFormat": "XRGB8888", "mirrorOf": "none",
             "availableModes": ["2880x1800@60.00Hz", "2880x1800@120.00Hz"],
             "colorManagementPreset": "srgb", "sdrBrightness": 1, "sdrSaturation": 1},
            {"name": "DP-1", "description": "ASUSTek COMPUTER INC PG27UCDM T5LMAV009458",
             "make": "ASUSTek COMPUTER INC", "model": "PG27UCDM", "width": 3840, "height": 2160,
             "physicalWidth": 590, "physicalHeight": 330, "refreshRate": 59.997, "x": 0, "y": 0,
             "activeWorkspace": {"id": 2}, "scale": 2, "transform": 0, "vrr": false, "disabled": false,
             "currentFormat": "XRGB2101010", "mirrorOf": "none",
             "availableModes": ["3840x2160@60.00Hz", "3840x2160@240.00Hz", "3840x2160@120.00Hz",
                                "3840x2160@119.88Hz", "3840x2160@120.00Hz", "3840x2160@59.94Hz",
                                "2560x1440@120.00Hz", "1920x1080@60.00Hz"],
             "colorManagementPreset": "srgb", "sdrBrightness": 1, "sdrSaturation": 1}
        ]);
        json.as_array()
            .unwrap()
            .iter()
            .map(|monitor| Display::from_json(monitor).unwrap())
            .collect()
    }
    #[test]
    fn displays_are_read_from_hyprctl() {
        let all = monitors();
        let (laptop, asus) = (&all[0], &all[1]);
        assert_eq!(laptop.title(), "Built-in Display");
        assert_eq!(asus.title(), "ASUSTek PG27UCDM");
        assert!(!laptop.enabled && asus.enabled && asus.ten_bit);
        assert_eq!(asus.logical_size(), (1920, 1080));
        assert_eq!(format!("{:.0}", asus.diagonal_inches().unwrap()), "27");
        assert_eq!(asus.refresh_rates(), [240.0, 120.0, 119.88, 60.0, 59.94]);
        assert_eq!(nearest(&asus.refresh_rates(), 59.997), Some(3));
        assert_eq!(
            asus.resolutions(),
            [(3840, 2160), (2560, 1440), (1920, 1080)]
        );
        assert_eq!(refresh_label(240.0), "240 Hz");
        assert_eq!(refresh_label(119.88), "119.88 Hz");
        assert_eq!(
            Mode::parse("3840x2160@240.00Hz").unwrap().setting(),
            "3840x2160@240.000"
        );
        // Off since the session began: no mode, so its preferred one, and a
        // change that names it stays valid.
        let off = Display::from_json(&json!({
            "name": "eDP-1", "width": 0, "height": 0, "refreshRate": 60.0, "x": -1, "y": -1,
            "scale": 1, "disabled": true,
            "availableModes": ["2880x1800@60.00Hz", "2880x1800@120.00Hz"]
        }))
        .unwrap();
        assert_eq!(off.mode.setting(), "2880x1800@60.000");
        assert_eq!(off.modes.len(), 2);
        assert_eq!(Settings::of(&off, 0).to_json()["mode"], "2880x1800@60.000");
    }
    #[test]
    fn scales_keep_whole_logical_pixels() {
        for scale in valid_scales(3840, 2160) {
            assert_eq!((3840.0 / scale).fract(), 0.0);
            assert_eq!((2160.0 / scale).fract(), 0.0);
        }
        assert_eq!(scale_choices(3840, 2160, 2.0), [3.0, 2.0, 1.6, 1.25, 1.0]);
        // The current scale is offered even when it is no preset.
        assert!(scale_choices(3840, 2160, 1.5).contains(&1.5));
        assert_eq!(clean_scale(1.3, 2880, 1800), 1.3333333333333333);
        assert_eq!(scale_label(2.0), "200%");
        assert_eq!(scale_label(1.0 + 1.0 / 3.0), "133.3%");
    }
    #[test]
    fn dropped_displays_share_an_edge_and_never_overlap() {
        let main = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let laptop = Rect {
            w: 1440,
            h: 900,
            ..main
        };
        // Dropped over the main display's right half: beside it, lined up
        // with its top.
        assert_eq!(
            snap(
                Rect {
                    x: 1500,
                    y: 30,
                    ..laptop
                },
                &[main],
                60
            ),
            (1920, 0)
        );
        // Dropped low on the left: beside it, its bottom lined up.
        assert_eq!(
            snap(
                Rect {
                    x: -1600,
                    y: 200,
                    ..laptop
                },
                &[main],
                60
            ),
            (-1440, 180)
        );
        // Dropped above: on top, centered when close to the middle.
        assert_eq!(
            snap(
                Rect {
                    x: 230,
                    y: -1200,
                    ..laptop
                },
                &[main],
                60
            ),
            (240, -900)
        );
        // Far away: brought back to touch it.
        assert_eq!(
            snap(
                Rect {
                    x: 9000,
                    y: 5000,
                    ..laptop
                },
                &[main],
                60
            ),
            (1920, 1079)
        );
        assert_eq!(snap(laptop, &[], 60), (0, 0));
        let arranged = normalized(&[
            ("DP-1".into(), main),
            (
                "eDP-1".into(),
                Rect {
                    x: -1440,
                    y: 180,
                    ..laptop
                },
            ),
        ]);
        assert_eq!(
            arranged,
            [("DP-1".to_owned(), 1440, 0), ("eDP-1".to_owned(), 0, 180)]
        );
    }
    #[test]
    fn changes_name_the_displays_they_change() {
        let all = monitors();
        let kept = json!({"main": "DP-1", "displays": {
            "desc:ASUSTek COMPUTER INC PG27UCDM T5LMAV009458": {"vrr": 2}
        }});
        assert_eq!(main_display(&all, &kept).as_deref(), Some("DP-1"));
        // The kept main display is off: the one on is the main display.
        assert_eq!(
            main_display(&all, &json!({"main": "eDP-1"})).as_deref(),
            Some("DP-1")
        );
        assert_eq!(kept_vrr(&kept, &all[1], &all), Some(2));
        let before: Vec<Settings> = all.iter().map(|display| Settings::of(display, 0)).collect();
        let mut after = before.clone();
        after[1].mode = Mode::parse("3840x2160@240.00Hz").unwrap();
        assert_eq!(changed(&before, &after), ["DP-1"]);
        let change = change_json(&after, Some("DP-1"), &changed(&before, &after));
        assert_eq!(change["displays"][1]["mode"], "3840x2160@240.000");
        assert_eq!(change["displays"][1]["position"], "0x0");
        assert_eq!(change["displays"][1]["bitdepth"], 10);
        assert_eq!(change["displays"][0]["enabled"], false);
        assert_eq!(change["apply"], json!(["DP-1"]));
    }
    fn two_on() -> (Vec<Settings>, Option<String>) {
        let mut settings: Vec<Settings> = monitors()
            .iter()
            .map(|display| Settings::of(display, 0))
            .collect();
        // The laptop on, at the right of the 4K display.
        settings[0].enabled = true;
        (settings[0].x, settings[0].y) = (1920, 0);
        (settings, Some("DP-1".to_owned()))
    }
    #[test]
    fn the_main_display_moves_when_it_stops_being_one() {
        let (mut settings, mut main) = two_on();
        use_as(&mut settings, &mut main, "DP-1", Use::Off);
        assert!(!settings[1].enabled);
        assert_eq!(main.as_deref(), Some("eDP-1"));
        // Alone now, the laptop starts the arrangement.
        assert_eq!((settings[0].x, settings[0].y), (0, 0));

        let (mut settings, mut main) = two_on();
        use_as(
            &mut settings,
            &mut main,
            "DP-1",
            Use::Mirror("eDP-1".into()),
        );
        assert_eq!(settings[1].mirror.as_deref(), Some("eDP-1"));
        assert_eq!(main.as_deref(), Some("eDP-1"));

        let (mut settings, mut main) = two_on();
        use_as(&mut settings, &mut main, "eDP-1", Use::Main);
        assert_eq!(main.as_deref(), Some("eDP-1"));
        use_as(&mut settings, &mut main, "eDP-1", Use::Extended);
        assert_eq!(main.as_deref(), Some("DP-1"));
    }
    #[test]
    fn a_display_turned_on_goes_beside_the_others() {
        let mut settings: Vec<Settings> = monitors()
            .iter()
            .map(|display| Settings::of(display, 0))
            .collect();
        let mut main = Some("DP-1".to_owned());
        // Off, it kept a position over the 4K display.
        (settings[0].x, settings[0].y) = (0, 0);
        use_as(&mut settings, &mut main, "eDP-1", Use::Extended);
        assert!(settings[0].enabled);
        assert!(!rect(&settings[0]).overlaps(&rect(&settings[1])));
        assert_eq!((settings[0].x, settings[0].y), (1920, 0));
        // A display that mirrored one turned off shows its own picture.
        let (mut settings, mut main) = two_on();
        settings[0].mirror = Some("DP-1".into());
        use_as(&mut settings, &mut main, "DP-1", Use::Off);
        assert!(settings[0].mirror.is_none() && settings[0].enabled);
        // ... and, the only display left, is the main one.
        assert_eq!(main.as_deref(), Some("eDP-1"));
    }
    #[test]
    fn neighbours_follow_a_display_that_changes_size() {
        let (mut settings, _) = two_on();
        let old = settings[1].logical_size();
        // The 4K display at 100%: twice as wide, the laptop moves right.
        settings[1].scale = 1.0;
        resized(&mut settings, "DP-1", old);
        assert_eq!((settings[0].x, settings[0].y), (3840, 0));
        // Dropped onto the 4K display, the laptop goes beside it.
        place(&mut settings, "eDP-1", 1000, 300);
        assert!(!rect(&settings[0]).overlaps(&rect(&settings[1])));
    }
}
