//! Hako's settings: `$XDG_CONFIG_HOME/aizome/hako.toml`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::fileview::Layout;
use crate::model::entry::Sort;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// How new windows show folders.
    pub layout: Layout,
    /// Show hidden files (those whose names start with a dot).
    pub hidden: bool,
    pub sort: Sort,
    /// Each column's width in the column view.
    pub column_width: f64,
    pub sidebar: f64,
    /// A new window's size.
    pub window: (f64, f64),
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            layout: Layout::Column,
            hidden: false,
            sort: Sort::default(),
            column_width: 220.0,
            sidebar: 196.0,
            window: (980.0, 600.0),
        }
    }
}

impl Settings {
    /// The settings at `path`, or the defaults.
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    /// Keep them at `path` (through a temporary file beside it).
    pub fn save(&self, path: &Path) {
        let Ok(text) = toml::to_string_pretty(self) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("toml.new");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults_what_is_missing() {
        let t = crate::fsops::testing::TempDir::new("settings");
        let p = t.join("hako.toml");
        assert_eq!(Settings::load(&p), Settings::default());
        let s = Settings { hidden: true, layout: Layout::List, ..Settings::default() };
        s.save(&p);
        assert_eq!(Settings::load(&p), s);
        std::fs::write(&p, "hidden = true\nunknown = 3\n").unwrap();
        let s = Settings::load(&p);
        assert!(s.hidden);
        assert_eq!(s.layout, Layout::Column);
    }
}
