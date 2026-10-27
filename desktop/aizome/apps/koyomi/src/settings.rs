//! Koyomi's settings (`$XDG_CONFIG_HOME/aizome/koyomi.toml`) and the
//! state it keeps between runs (`$XDG_STATE_HOME/aizome/koyomi/`).

use std::path::PathBuf;

use jiff::civil::Weekday;
use serde::{Deserialize, Serialize};

use crate::locale::{self, Weeks};

/// Where the calendars are: `$XDG_DATA_HOME/aizome/koyomi`.
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("aizome/koyomi")
}

/// `$XDG_STATE_HOME/aizome/koyomi`.
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("aizome/koyomi")
}

/// `$XDG_CONFIG_HOME/aizome/koyomi.toml`.
pub fn settings_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("aizome/koyomi.toml")
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Which day weeks start on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FirstDay {
    /// As the locale has it.
    #[default]
    Locale,
    Monday,
    Sunday,
    Saturday,
}

/// How times read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Clock {
    /// As the menu bar's clock reads (Settings ▸ Menu Bar).
    #[default]
    System,
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "12h")]
    H12,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub first_day: FirstDay,
    pub week_numbers: bool,
    pub clock: Clock,
    /// Minutes before a new timed event its alert goes off (none: no
    /// alert).
    pub default_alert: Option<i64>,
    /// Minutes before midnight of an all-day event's day: 900 is 09:00
    /// the day before.
    pub default_all_day_alert: Option<i64>,
    /// How long Snooze puts an alert off.
    pub snooze_minutes: i64,
    /// The hour the week and day views show first.
    pub day_starts: u8,
    /// The calendar new events go in (its id).
    pub default_calendar: Option<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            first_day: FirstDay::Locale,
            week_numbers: true,
            clock: Clock::System,
            default_alert: Some(10),
            default_all_day_alert: Some(900),
            snooze_minutes: 10,
            day_starts: 8,
            default_calendar: None,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(settings_path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_path();
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&path, text.as_bytes())
    }

    /// The weeks to show, the locale's unless the first day is set.
    pub fn weeks(&self, locale: &Weeks) -> Weeks {
        let first_day = match self.first_day {
            FirstDay::Locale => locale.first_day,
            FirstDay::Monday => Weekday::Monday,
            FirstDay::Sunday => Weekday::Sunday,
            FirstDay::Saturday => Weekday::Saturday,
        };
        Weeks { first_day, min_days: locale.min_days }
    }

    /// Whether times read 24-hour: as set, else as the menu bar's clock
    /// does, else as the locale does.
    pub fn uses_24h(&self, desktop: Option<bool>) -> bool {
        match self.clock {
            Clock::H24 => true,
            Clock::H12 => false,
            Clock::System => desktop.unwrap_or_else(|| locale::uses_24h(&locale::time_locale())),
        }
    }
}

/// The desktop's clock: whether the menu bar reads 24-hour.
pub fn desktop_24h() -> Option<bool> {
    // The menu bar reads 24-hour unless set otherwise, settings file or not.
    let s = aizome_config::Settings::load_or_default();
    Some(s.menubar_settings().clock == aizome_config::shell::ClockFormat::H24)
}

/// What the window remembers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub view: Option<String>,
    /// Calendars hidden (their ids).
    pub hidden: Vec<String>,
    pub sidebar: Option<f64>,
    pub window: Option<(f64, f64)>,
}

impl State {
    pub fn path() -> PathBuf {
        state_dir().join("state.json")
    }

    pub fn load() -> State {
        std::fs::read_to_string(State::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = State::path();
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        write_atomic(&path, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?.as_bytes())
    }
}

/// Write beside and rename over, so a crash leaves the old file whole.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_what_they_know() {
        let s: Settings =
            toml::from_str("first_day = \"sunday\"\nclock = \"12h\"\nweek_numbers = false\nunknown = 3\n").unwrap();
        assert_eq!(s.first_day, FirstDay::Sunday);
        assert_eq!(s.clock, Clock::H12);
        assert!(!s.week_numbers);
        assert_eq!(s.snooze_minutes, 10);
        assert_eq!(s.weeks(&Weeks::default()).first_day, Weekday::Sunday);
        assert!(!s.uses_24h(Some(true)));
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
