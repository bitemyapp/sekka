//! Keys of `settings.toml` that `aizome-config` keeps as plain tables (the
//! shell's sections, and sections of Settings' own), read with defaults
//! and written in place, every other key kept.
//!
//! The keys Settei writes that aren't typed in `aizome-config` yet, for
//! the shell and the compositor to honour:
//!
//! | Section | Key | Meaning |
//! |---|---|---|
//! | `[notifications]` | `previews_when_locked` (bool, false) | notifications show their text on the lock screen |
//! | `[notifications]` | `blocked_apps` (list) | apps whose notifications are dropped |
//! | `[notifications]` | `silent_apps` (list) | apps whose notifications make no sound |
//! | `[deskbar]` | `show_shelf` (bool, true) | the Shelf under the apps |
//! | `[desktop]` | `show_home`, `show_disks`, `show_trash` (bool, true) | the desktop's icons |
//! | `[windows]` | `tab_double_click` (`"fill"`, `"minimise"`, `"nothing"`) | what double-clicking a tab does |
//! | `[lock]` | `show_clock` (bool, true), `after_monitors_off` (bool, true) | |
//! | `[power]` | `power_key` (`"ask"`, `"sleep"`, `"lock"`, `"off"`, `"nothing"`) | what the power key does; the shell holds logind's `handle-power-key` while it's set |
//! | `[palette]` | `scopes` (list), `exclude` (list) | the palette's scopes; folders the file index skips |
//! | `[performance]` | `renderer` (`"auto"`, `"gpu"`, `"software"`) | the compositor's renderer at the next login |
//! | `[input.pointer]`, `[input.touchpad]` | `scroll_factor` (float, 1.0) | scrolling distance |
//! | `[input.keyboard]` | `sticky_keys`, `slow_keys` (bool), `slow_keys_ms` | AccessX |
//! | `[sound]` | `visual_alert` (bool), `alert_sound` (a sound theme name), `alert_volume` (0–1), `volume_feedback` (bool) | the alert the shell plays for the system bell (and notifications' sounds at its volume), the screen's edge flashing too; a sound at the new volume when the keys or the Sound panel change it |

use aizome_config::Settings;

pub fn get_bool(t: &toml::Table, key: &str, default: bool) -> bool {
    t.get(key).and_then(toml::Value::as_bool).unwrap_or(default)
}

pub fn get_str<'a>(t: &'a toml::Table, key: &str) -> Option<&'a str> {
    t.get(key).and_then(toml::Value::as_str)
}

pub fn get_int(t: &toml::Table, key: &str, default: i64) -> i64 {
    t.get(key).and_then(toml::Value::as_integer).unwrap_or(default)
}

pub fn get_list(t: &toml::Table, key: &str) -> Option<Vec<String>> {
    t.get(key).and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
}

pub fn set(t: &mut toml::Table, key: &str, value: impl Into<toml::Value>) {
    t.insert(key.to_owned(), value.into());
}

pub fn set_list(t: &mut toml::Table, key: &str, list: &[String]) {
    t.insert(key.to_owned(), toml::Value::Array(list.iter().map(|s| toml::Value::String(s.clone())).collect()));
}

/// A section of its own kept among the top level's extra keys
/// (`[desktop]`, `[performance]`, `[palette]`), made if missing.
pub fn section<'a>(s: &'a mut Settings, name: &str) -> &'a mut toml::Table {
    let v = s.extra.entry(name.to_owned()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !v.is_table() {
        *v = toml::Value::Table(toml::Table::new());
    }
    v.as_table_mut().expect("a table")
}

/// A section of the top level's extra keys, to read (empty if missing).
pub fn section_ref<'a>(s: &'a Settings, name: &str) -> &'a toml::Table {
    static EMPTY: std::sync::OnceLock<toml::Table> = std::sync::OnceLock::new();
    s.extra.get(name).and_then(toml::Value::as_table).unwrap_or_else(|| EMPTY.get_or_init(toml::Table::new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_of_our_own_survive_a_round_trip() {
        let mut s = Settings::default();
        set(section(&mut s, "desktop"), "show_trash", false);
        set_list(&mut s.notifications, "blocked_apps", &["firefox.desktop".to_owned()]);
        let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
        assert!(!get_bool(section_ref(&back, "desktop"), "show_trash", true));
        assert_eq!(get_list(&back.notifications, "blocked_apps").unwrap(), ["firefox.desktop"]);
        assert!(get_bool(section_ref(&back, "nothing"), "x", true));
    }
}
