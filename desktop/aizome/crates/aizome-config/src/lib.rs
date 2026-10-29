//! Aizome's settings: `~/.config/aizome/settings.toml`.
//!
//! Every part of Aizome reads the same file through this crate. Missing keys
//! take their defaults, and keys this version doesn't know are kept (each
//! section carries an `extra` table), so a load followed by a save never
//! loses what another version or a hand edit put there.
//!
//! The sections the compositor owns are typed here. The shell's and the
//! Settings app's sections (`menubar`, `deskbar`, `notifications`, `lock`,
//! `power`, `apps`, `sound`) are plain tables that their owners interpret.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

pub use aizome_theme::{Effects, FocusColor, ThemeName};

pub mod compositor;
pub mod greeter;
pub mod location;
pub mod shell;
pub use compositor::{NightLight, NightSchedule, RendererChoice, Sun, TabDoubleClick, sun_times, temperature_rgb};
pub use location::{LocalNow, Location, Place, PlaceSource, Span, daylight, hm, system_timezone};
use serde::{Deserialize, Serialize};
pub use shell::{
    ClockFormat, DESKBAR_SIZES, DeskbarSettings, DeskbarSide, DesktopSettings, LockSettings, MenubarSettings,
    NotificationSettings, PALETTE_SCOPES, PaletteSettings, PowerKey, PowerSettings, Recorder, RecordingFormat,
    RecordingQuality, RecordingSettings, SYSTRAY_WIDGETS, SoundSettings, names_app,
};

/// Everything in `settings.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub input: Input,
    /// Displays, matched by make, model and serial (or connector name).
    pub outputs: Vec<OutputConfig>,
    /// The Desks, in order. Empty means the four defaults.
    pub desks: Vec<DeskConfig>,
    pub keys: Keys,
    pub windows: Windows,
    pub idle: Idle,
    /// Warmer colours at night.
    pub night_light: NightLight,
    /// Where the computer is, for sunrise and sunset (the shell writes
    /// what GeoClue finds); left out of the file while it's empty.
    #[serde(skip_serializing_if = "Location::is_empty")]
    pub location: Location,
    pub menubar: toml::Table,
    pub deskbar: toml::Table,
    pub notifications: toml::Table,
    pub lock: toml::Table,
    pub power: toml::Table,
    pub apps: toml::Table,
    pub sound: toml::Table,
    /// Top-level keys this version doesn't know.
    #[serde(flatten)]
    pub extra: toml::Table,
}

/// Which palette to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeChoice {
    #[default]
    Night,
    Day,
    HighContrast,
    /// Day between sunrise and sunset, Night otherwise.
    Auto,
}

/// Auto's day by the clock alone: 07:00 to 19:00.
pub const CLOCK_DAY: Span = Span::Between { start: 7 * 60, end: 19 * 60 };

impl ThemeChoice {
    /// The palette at a local hour, Auto by the clock alone (Day from 07:00
    /// to 19:00). [`Settings::theme_now`] follows the sun where asked.
    pub fn resolve(self, local_hour: u32) -> ThemeName {
        self.resolve_in(CLOCK_DAY, local_hour * 60)
    }

    /// The palette at `minutes` past midnight, Auto showing Day during
    /// `day`.
    pub fn resolve_in(self, day: Span, minutes: u32) -> ThemeName {
        match self {
            ThemeChoice::Night => ThemeName::Night,
            ThemeChoice::Day => ThemeName::Day,
            ThemeChoice::HighContrast => ThemeName::HighContrast,
            ThemeChoice::Auto if day.contains(minutes) => ThemeName::Day,
            ThemeChoice::Auto => ThemeName::Night,
        }
    }
}

impl Settings {
    /// When Auto shows Day on a day: from sunrise to sunset where the
    /// computer is when it follows them (and Aizome knows where that is),
    /// else 07:00 to 19:00; with the place it was reckoned for.
    pub fn auto_day(&self, now: LocalNow) -> (Span, Option<Place>) {
        if !self.appearance.follow_sunset {
            return (CLOCK_DAY, None);
        }
        let place = self.place();
        match daylight(place.as_ref(), now.day_of_year, now.utc_offset) {
            Some(span) => (span, place),
            None => (CLOCK_DAY, None),
        }
    }

    /// The palette at a moment.
    pub fn theme_at(&self, now: LocalNow) -> ThemeName {
        let day = if self.appearance.theme == ThemeChoice::Auto { self.auto_day(now).0 } else { CLOCK_DAY };
        self.appearance.theme.resolve_in(day, now.minutes)
    }

    /// The palette to show now.
    pub fn theme_now(&self) -> ThemeName {
        self.theme_at(LocalNow::now())
    }
}

/// How large the interface is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiSize {
    Small,
    #[default]
    Medium,
    Large,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeChoice,
    /// Switch between Night and Day at sunset and sunrise (with `auto`).
    pub follow_sunset: bool,
    pub focus: FocusColor,
    pub effects: Effects,
    pub size: UiSize,
    /// No motion anywhere: layers jump instead of sliding.
    pub reduced_motion: bool,
    pub cursor_theme: String,
    pub cursor_size: u32,
    /// Unset follows the system wallpaper; an empty string means no picture.
    pub wallpaper_night: Option<String>,
    /// Unset follows the system wallpaper; an empty string means no picture.
    pub wallpaper_day: Option<String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: ThemeChoice::Night,
            follow_sunset: false,
            focus: FocusColor::Amber,
            effects: Effects::Lean,
            size: UiSize::Medium,
            reduced_motion: false,
            cursor_theme: "Adwaita".into(),
            cursor_size: 24,
            wallpaper_night: None,
            wallpaper_day: None,
            extra: toml::Table::new(),
        }
    }
}

impl Appearance {
    /// Resolve an explicit picture or the system default. An explicit empty
    /// value disables the picture, even when the session supplies a default.
    pub fn wallpaper_path(&self, day: bool) -> Option<PathBuf> {
        let configured = if day { &self.wallpaper_day } else { &self.wallpaper_night };
        match configured.as_deref() {
            Some("") => None,
            Some(path) => Some(expand_home(path)),
            None => Self::system_wallpaper(day),
        }
    }

    /// The wallpaper supplied by the session, before any user preference.
    pub fn system_wallpaper(day: bool) -> Option<PathBuf> {
        std::env::var_os(if day { "AIZOME_WALLPAPER_DAY" } else { "AIZOME_WALLPAPER_NIGHT" })
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    }

    /// How long layers take to move, after reduced motion.
    pub fn motion_ms(&self) -> u32 {
        if self.reduced_motion { 0 } else { self.effects.motion_ms() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Input {
    pub keyboard: Keyboard,
    pub pointer: Pointer,
    pub touchpad: Touchpad,
    #[serde(flatten)]
    pub extra: toml::Table,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keyboard {
    /// XKB layouts, e.g. `["us", "jp"]`.
    pub layouts: Vec<String>,
    /// XKB variants, one per layout (or empty).
    pub variant: String,
    /// XKB options, e.g. `"compose:ralt,ctrl:nocaps"`.
    pub options: String,
    pub model: String,
    /// Milliseconds before a held key repeats.
    pub repeat_delay: u32,
    /// Repeats per second.
    pub repeat_rate: u32,
    pub numlock: bool,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Keyboard {
    fn default() -> Self {
        Keyboard {
            layouts: vec!["us".into()],
            variant: String::new(),
            options: String::new(),
            model: String::new(),
            repeat_delay: 600,
            repeat_rate: 25,
            numlock: false,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccelProfile {
    #[default]
    Adaptive,
    Flat,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pointer {
    pub accel_profile: AccelProfile,
    /// -1.0 (slowest) to 1.0 (fastest).
    pub speed: f64,
    pub natural_scroll: bool,
    pub left_handed: bool,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Pointer {
    fn default() -> Self {
        Pointer {
            accel_profile: AccelProfile::Adaptive,
            speed: 0.0,
            natural_scroll: false,
            left_handed: false,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClickMethod {
    /// Two fingers right-click, three middle-click (the Mac's way).
    #[default]
    Clickfinger,
    /// The bottom corners of the pad are the buttons.
    ButtonAreas,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Touchpad {
    pub tap: bool,
    pub natural_scroll: bool,
    pub disable_while_typing: bool,
    pub click_method: ClickMethod,
    pub accel_profile: AccelProfile,
    pub speed: f64,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Touchpad {
    fn default() -> Self {
        Touchpad {
            tap: true,
            natural_scroll: true,
            disable_while_typing: true,
            click_method: ClickMethod::Clickfinger,
            accel_profile: AccelProfile::Adaptive,
            speed: 0.0,
            extra: toml::Table::new(),
        }
    }
}

/// An output's rotation and reflection, as wl_output names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transform {
    #[default]
    Normal,
    #[serde(rename = "90")]
    _90,
    #[serde(rename = "180")]
    _180,
    #[serde(rename = "270")]
    _270,
    Flipped,
    #[serde(rename = "flipped-90")]
    Flipped90,
    #[serde(rename = "flipped-180")]
    Flipped180,
    #[serde(rename = "flipped-270")]
    Flipped270,
}

/// One display's settings. It applies to the output whose make, model and
/// serial all match (missing ones match anything), or whose connector is
/// `name`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub name: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    /// `"2880x1800"` or `"2880x1800@60"` (refresh in Hz, decimals allowed).
    pub mode: Option<String>,
    /// 1.0, 2.0… Fractional values only when asked for.
    pub scale: Option<f64>,
    /// Top-left corner in the global layout, in points.
    pub position: Option<[i32; 2]>,
    pub transform: Option<Transform>,
    pub enabled: Option<bool>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl OutputConfig {
    /// Whether this entry applies to an output.
    pub fn matches(&self, name: &str, make: &str, model: &str, serial: &str) -> bool {
        if let Some(n) = &self.name {
            if n == name {
                return true;
            }
            if self.make.is_none() && self.model.is_none() && self.serial.is_none() {
                return false;
            }
        }
        let f = |want: &Option<String>, have: &str| want.as_deref().is_none_or(|w| w == have);
        (self.make.is_some() || self.model.is_some() || self.serial.is_some())
            && f(&self.make, make)
            && f(&self.model, model)
            && f(&self.serial, serial)
    }

    /// The mode as width, height and refresh in millihertz (if given).
    pub fn parsed_mode(&self) -> Option<(i32, i32, Option<i32>)> {
        parse_mode(self.mode.as_deref()?)
    }
}

/// Parses `"WxH"` or `"WxH@R"` (R in Hz).
pub fn parse_mode(s: &str) -> Option<(i32, i32, Option<i32>)> {
    let (size, refresh) = match s.split_once('@') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let (w, h) = size.trim().split_once(['x', 'X'])?;
    let w = w.trim().parse().ok()?;
    let h = h.trim().parse().ok()?;
    let r = match refresh {
        Some(r) => Some((r.trim().trim_end_matches("Hz").parse::<f64>().ok()? * 1000.0).round() as i32),
        None => None,
    };
    Some((w, h, r))
}

/// One Desk: a name and, optionally, the folder its terminal starts in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeskConfig {
    pub name: String,
    pub folder: Option<String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl DeskConfig {
    pub fn named(name: impl Into<String>) -> DeskConfig {
        DeskConfig { name: name.into(), ..Default::default() }
    }
}

/// The Desks to start with when the file names none.
pub fn default_desks() -> Vec<DeskConfig> {
    (1..=4).map(|n| DeskConfig::named(n.to_string())).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keys {
    /// Translate ❖-shortcuts into the Ctrl ones apps that don't use
    /// Aizome's kit expect (Ctrl+Shift in terminals).
    pub command_key_for_all_apps: bool,
    /// App IDs that get Ctrl+Shift instead of Ctrl.
    pub terminals: Vec<String>,
    /// What ❖Return runs, in the Desk's folder.
    pub terminal: Vec<String>,
    /// Overrides: a chord (`"Ctrl+Super+Up"`) to an action name
    /// (`"overview"`, `"spawn foot"`, `"none"` to unbind). See the
    /// compositor's documentation for the action names.
    pub bindings: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            command_key_for_all_apps: true,
            terminals: default_terminals(),
            terminal: vec!["foot".into()],
            bindings: BTreeMap::new(),
            extra: toml::Table::new(),
        }
    }
}

pub fn default_terminals() -> Vec<String> {
    [
        "foot",
        "footclient",
        "kitty",
        "Alacritty",
        "org.wezfurlong.wezterm",
        "com.mitchellh.ghostty",
        "org.gnome.Console",
        "org.gnome.Terminal",
        "org.gnome.Ptyxis",
        "org.kde.konsole",
        "xterm",
        "XTerm",
        "st",
        "st-256color",
        "urxvt",
        "rio",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FocusFollows {
    /// Click to focus (the default).
    #[default]
    Click,
    /// Focus follows the pointer, without raising.
    Mouse,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Windows {
    /// A new window of the focused app joins its tab group.
    pub stack_new_windows_of_same_app: bool,
    pub focus_follows: FocusFollows,
    /// Reopen windows where their app last left them.
    pub remember_geometry: bool,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Windows {
    fn default() -> Self {
        Windows {
            stack_new_windows_of_same_app: false,
            focus_follows: FocusFollows::Click,
            remember_geometry: true,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Idle {
    /// Minutes without input before the shell locks; 0 never.
    pub lock_after_minutes: u32,
    /// Minutes without input before displays power off; 0 never.
    pub monitors_off_after_minutes: u32,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Idle {
    fn default() -> Self {
        Idle { lock_after_minutes: 10, monitors_off_after_minutes: 15, extra: toml::Table::new() }
    }
}

/// Why the settings couldn't be read.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Parse(e) => write!(f, "{e}"),
            Error::Serialize(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl Settings {
    /// `$XDG_CONFIG_HOME/aizome/settings.toml`, or `~/.config/…`.
    pub fn path() -> PathBuf {
        config_dir().join("aizome").join("settings.toml")
    }

    /// Reads the settings file. A missing file gives the defaults.
    pub fn load() -> Result<Settings, Error> {
        Settings::load_from(&Settings::path())
    }

    pub fn load_from(path: &Path) -> Result<Settings, Error> {
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::from_toml(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Reads the settings, falling back to the defaults (with a warning in
    /// the log) when the file is broken.
    pub fn load_or_default() -> Settings {
        Settings::load().unwrap_or_else(|e| {
            log::warn!("{}: {e}; using the defaults", Settings::path().display());
            Settings::default()
        })
    }

    pub fn from_toml(text: &str) -> Result<Settings, Error> {
        toml::from_str(text).map_err(Error::Parse)
    }

    pub fn to_toml(&self) -> Result<String, Error> {
        toml::to_string_pretty(self).map_err(Error::Serialize)
    }

    /// Writes the settings file (atomically, through a temporary file).
    pub fn save(&self) -> Result<(), Error> {
        self.save_to(&Settings::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), Error> {
        let text = self.to_toml()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// `[menubar]`, typed (see [`shell`]).
    pub fn menubar_settings(&self) -> MenubarSettings {
        shell::read(Some(&self.menubar))
    }

    /// `[deskbar]`, typed.
    pub fn deskbar_settings(&self) -> DeskbarSettings {
        shell::read(Some(&self.deskbar))
    }

    /// `[notifications]`, typed.
    pub fn notification_settings(&self) -> NotificationSettings {
        shell::read(Some(&self.notifications))
    }

    /// `[lock]`, typed.
    pub fn lock_settings(&self) -> LockSettings {
        shell::read(Some(&self.lock))
    }

    /// `[palette]`, typed (kept among the top level's extra keys).
    pub fn palette_settings(&self) -> PaletteSettings {
        shell::read(self.extra.get("palette").and_then(|v| v.as_table()))
    }

    /// `[desktop]`, typed (kept among the top level's extra keys).
    pub fn desktop_settings(&self) -> DesktopSettings {
        shell::read(self.extra.get("desktop").and_then(|v| v.as_table()))
    }

    /// `[power]`, typed.
    pub fn power_settings(&self) -> PowerSettings {
        shell::read(Some(&self.power))
    }

    /// `[sound]`'s keys for the shell, typed.
    pub fn sound_settings(&self) -> SoundSettings {
        shell::read(Some(&self.sound))
    }

    /// `[recording]`, typed (kept among the top level's extra keys).
    pub fn recording_settings(&self) -> RecordingSettings {
        shell::read(self.extra.get("recording").and_then(|v| v.as_table()))
    }

    /// Write one of the shell's sections (`"menubar"`, `"deskbar"`,
    /// `"notifications"`, `"lock"`, `"palette"`, `"desktop"`, `"power"` or
    /// `"sound"`) over its table, keeping the keys it doesn't know.
    pub fn set_shell_section<T: Serialize>(&mut self, section: &str, value: &T) {
        let table = match section {
            "menubar" => &mut self.menubar,
            "deskbar" => &mut self.deskbar,
            "notifications" => &mut self.notifications,
            "lock" => &mut self.lock,
            "power" => &mut self.power,
            "sound" => &mut self.sound,
            other => {
                let v = self.extra.entry(other.to_owned()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
                if !v.is_table() {
                    *v = toml::Value::Table(toml::Table::new());
                }
                v.as_table_mut().expect("a table")
            }
        };
        shell::write(table, value);
    }

    /// The Desks to use: the configured ones, or the four defaults.
    pub fn desks_or_default(&self) -> Vec<DeskConfig> {
        if self.desks.is_empty() { default_desks() } else { self.desks.clone() }
    }

    /// The output entry that applies to an output, if any.
    pub fn output(&self, name: &str, make: &str, model: &str, serial: &str) -> Option<&OutputConfig> {
        self.outputs.iter().find(|o| o.matches(name, make, model, serial))
    }
}

/// `$XDG_CONFIG_HOME`, or `~/.config`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"))
}

/// `$XDG_STATE_HOME/aizome`, or `~/.local/state/aizome`: where the
/// compositor remembers window positions.
pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("state"))
        .join("aizome")
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Expands a leading `~` to the home directory.
pub fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        home_dir()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// Watches the settings file and calls back when it changes.
///
/// It polls the file's modification time once a second on its own thread,
/// which costs nothing measurable and needs no inotify plumbing; dropping
/// the watcher stops the thread.
pub struct Watcher {
    stop: Arc<AtomicBool>,
}

impl Watcher {
    /// Watches `Settings::path()`.
    pub fn spawn(on_change: impl FnMut() + Send + 'static) -> Watcher {
        Watcher::spawn_for(Settings::path(), Duration::from_secs(1), on_change)
    }

    pub fn spawn_for(path: PathBuf, interval: Duration, mut on_change: impl FnMut() + Send + 'static) -> Watcher {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let stamp = |p: &Path| -> Option<(SystemTime, u64)> {
            let m = std::fs::metadata(p).ok()?;
            Some((m.modified().ok()?, m.len()))
        };
        std::thread::Builder::new()
            .name("aizome-config-watch".into())
            .spawn(move || {
                let mut last = stamp(&path);
                while !stop2.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    let now = stamp(&path);
                    if now != last {
                        last = now;
                        on_change();
                    }
                }
            })
            .expect("spawn the settings watcher");
        Watcher { stop }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        let s = Settings::from_toml("").unwrap();
        assert_eq!(s, Settings::default());
        assert_eq!(s.desks_or_default().len(), 4);
        assert_eq!(s.desks_or_default()[0].name, "1");
        assert!(s.keys.command_key_for_all_apps);
        assert_eq!(s.appearance.cursor_theme, "Adwaita");
        assert_eq!(s.appearance.motion_ms(), 80);
    }

    #[test]
    fn missing_keys_take_defaults() {
        let s = Settings::from_toml("[appearance]\ntheme = \"day\"\n[input.keyboard]\nlayouts = [\"us\", \"jp\"]\n")
            .unwrap();
        assert_eq!(s.appearance.theme, ThemeChoice::Day);
        assert_eq!(s.appearance.focus, FocusColor::Amber);
        assert_eq!(s.input.keyboard.layouts, ["us", "jp"]);
        assert_eq!(s.input.keyboard.repeat_rate, 25);
    }

    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let text = r#"
future_top = 3
[appearance]
theme = "high-contrast"
sparkle = true
[input.touchpad]
tap = false
pressure = 0.5
[[desks]]
name = "Mail"
folder = "~/Mail"
colour = "red"
[[outputs]]
make = "BOE"
mode = "2880x1800@60"
scale = 2.0
hdr = false
[menubar]
clock = "24h"
[newsection]
a = 1
"#;
        let s = Settings::from_toml(text).unwrap();
        assert_eq!(s.appearance.theme, ThemeChoice::HighContrast);
        assert!(!s.input.touchpad.tap);
        let again = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
        assert_eq!(s, again);
        assert_eq!(again.extra.get("future_top").and_then(|v| v.as_integer()), Some(3));
        assert_eq!(again.extra.get("newsection").and_then(|v| v.get("a")).and_then(|v| v.as_integer()), Some(1));
        assert_eq!(again.appearance.extra.get("sparkle").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(again.input.touchpad.extra.get("pressure").and_then(|v| v.as_float()), Some(0.5));
        assert_eq!(again.desks[0].extra.get("colour").and_then(|v| v.as_str()), Some("red"));
        assert_eq!(again.outputs[0].extra.get("hdr").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(again.menubar.get("clock").and_then(|v| v.as_str()), Some("24h"));
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("aizome-config-test-{}", std::process::id()));
        let path = dir.join("settings.toml");
        let mut s = Settings::default();
        s.windows.stack_new_windows_of_same_app = true;
        s.desks = vec![
            DeskConfig::named("Mail"),
            DeskConfig { name: "Build".into(), folder: Some("~/work".into()), ..Default::default() },
        ];
        s.save_to(&path).unwrap();
        let back = Settings::load_from(&path).unwrap();
        assert_eq!(back, s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wallpaper_blank_choices_survive_serialization_and_custom_paths_stay_per_theme() {
        let settings =
            Settings::from_toml("[appearance]\nwallpaper_night = \"\"\nwallpaper_day = \"~/Pictures/day.png\"\n")
                .unwrap();
        assert_eq!(settings.appearance.wallpaper_path(false), None);
        assert_eq!(settings.appearance.wallpaper_path(true), Some(home_dir().join("Pictures/day.png")));
        let serialized = toml::to_string(&settings).unwrap();
        let again = Settings::from_toml(&serialized).unwrap();
        assert_eq!(again.appearance.wallpaper_night.as_deref(), Some(""));
        assert_eq!(again.appearance.wallpaper_day.as_deref(), Some("~/Pictures/day.png"));
        assert!(Settings::default().appearance.wallpaper_night.is_none(), "unset remains the system default");
    }

    #[test]
    fn output_matching_and_modes() {
        let o = OutputConfig { make: Some("BOE".into()), model: Some("NE135".into()), ..Default::default() };
        assert!(o.matches("eDP-1", "BOE", "NE135", "123"));
        assert!(!o.matches("eDP-1", "BOE", "Other", "123"));
        let n = OutputConfig { name: Some("HDMI-A-1".into()), ..Default::default() };
        assert!(n.matches("HDMI-A-1", "", "", ""));
        assert!(!n.matches("DP-1", "", "", ""));
        assert_eq!(parse_mode("2880x1800@60"), Some((2880, 1800, Some(60000))));
        assert_eq!(parse_mode("1920x1080@59.94"), Some((1920, 1080, Some(59940))));
        assert_eq!(parse_mode("1280x800"), Some((1280, 800, None)));
        assert_eq!(parse_mode("nonsense"), None);
    }

    #[test]
    fn transforms_parse() {
        let s = Settings::from_toml("[[outputs]]\nname = \"DP-1\"\ntransform = \"90\"\n").unwrap();
        assert_eq!(s.outputs[0].transform, Some(Transform::_90));
    }

    #[test]
    fn auto_theme_follows_the_clock() {
        assert_eq!(ThemeChoice::Auto.resolve(12), ThemeName::Day);
        assert_eq!(ThemeChoice::Auto.resolve(22), ThemeName::Night);
    }

    #[test]
    fn auto_theme_follows_the_sun_when_asked() {
        let mut s = Settings::default();
        s.appearance.theme = ThemeChoice::Auto;
        // Paris on 1 November at 18:00, CET: past sunset (about 17:25).
        s.night_light.latitude = Some(48.8667);
        s.night_light.longitude = Some(2.3333);
        let evening = LocalNow::at(18, 0, 305, 60);
        assert_eq!(s.theme_at(evening), ThemeName::Day, "by the clock, Day until 19:00");
        s.appearance.follow_sunset = true;
        assert_eq!(s.theme_at(evening), ThemeName::Night);
        assert_eq!(s.theme_at(LocalNow::at(7, 15, 305, 60)), ThemeName::Night, "before sunrise (about 07:30)");
        assert_eq!(s.theme_at(LocalNow::at(8, 0, 305, 60)), ThemeName::Day);
        let (Span::Between { start, end }, Some(place)) = s.auto_day(evening) else { panic!() };
        assert_eq!(place.source, PlaceSource::Settings);
        assert!(start > 7 * 60 && end < 18 * 60, "{} {}", hm(start), hm(end));
        // Tromsø in June: the sun never sets, so Day at midnight.
        s.night_light.latitude = Some(69.65);
        s.night_light.longitude = Some(18.96);
        assert_eq!(s.theme_at(LocalNow::at(0, 30, 172, 120)), ThemeName::Day);
        // Other themes stay as chosen.
        s.appearance.theme = ThemeChoice::Night;
        assert_eq!(s.theme_at(LocalNow::at(12, 0, 172, 120)), ThemeName::Night);
    }
}
