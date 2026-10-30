//! Nisshi's settings, `$XDG_CONFIG_HOME/aizome/nisshi.toml`: what the
//! View menu and the toolbar set and Nisshi keeps between launches.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The newest message at the top of the list.
    pub newest_first: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { newest_first: true }
    }
}

pub fn default_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("aizome/nisshi.toml")
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    /// Write them, replacing the file whole (a new file, then renamed).
    pub fn save(&self, path: &Path) {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let Ok(text) = toml::to_string(self) else { return };
        let fresh = path.with_extension("toml.new");
        if let Err(e) = std::fs::write(&fresh, text).and_then(|()| std::fs::rename(&fresh, path)) {
            log::warn!("{}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_and_read_again() {
        let dir = std::env::temp_dir().join(format!("nisshi-settings-{}", std::process::id()));
        let path = dir.join("aizome/nisshi.toml");
        assert!(Settings::load(&path).newest_first, "newest first, before anything is kept");
        Settings { newest_first: false }.save(&path);
        assert_eq!(Settings::load(&path), Settings { newest_first: false });
        std::fs::write(&path, "not = [toml").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
