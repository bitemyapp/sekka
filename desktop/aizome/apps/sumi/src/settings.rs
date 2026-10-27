//! Sumi's settings, `$XDG_CONFIG_HOME/aizome/sumi.toml`: what the View
//! menu and Format ▸ Font set.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The text's size at "Actual Size", as the board's editor sets it.
pub const DEFAULT_SIZE: f64 = 13.0;
pub const SIZES: [f64; 11] = [9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub line_numbers: bool,
    pub wrap: bool,
    /// The text's size, in points.
    pub size: f64,
    /// Archivo rather than IBM Plex Mono.
    pub proportional: bool,
    /// Tab inserts this many spaces (0: a tab).
    pub soft_tabs: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { line_numbers: true, wrap: true, size: DEFAULT_SIZE, proportional: false, soft_tabs: 0 }
    }
}

pub fn default_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("aizome/sumi.toml")
}

impl Settings {
    pub fn load(path: Option<&Path>) -> Settings {
        let mut s: Settings = path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        if !(6.0..=72.0).contains(&s.size) {
            s.size = DEFAULT_SIZE;
        }
        s.soft_tabs = s.soft_tabs.min(8);
        s
    }

    pub fn save(&self, path: Option<&Path>) {
        let Some(p) = path else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(t) = toml::to_string(self)
            && let Err(e) = crate::files::save_atomic(p, t.as_bytes())
        {
            log::warn!("{}: {e}", p.display());
        }
    }

    /// The next size up (or down).
    pub fn zoomed(&self, bigger: bool) -> f64 {
        if bigger {
            SIZES.iter().copied().find(|s| *s > self.size + 0.01).unwrap_or(self.size)
        } else {
            SIZES.iter().rev().copied().find(|s| *s < self.size - 0.01).unwrap_or(self.size)
        }
    }
}
