//! What Annai remembers: whether it has been shown, and whether it shows
//! at every login. `$XDG_STATE_HOME/aizome/annai/state.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// When it was first shown, seconds since 1970 (none: never).
    pub shown: Option<u64>,
    /// Show it at every login, not only the first.
    pub show_at_login: bool,
}

impl State {
    /// Whether `--first-run` shows the window.
    pub fn due(&self) -> bool {
        self.shown.is_none() || self.show_at_login
    }
}

/// `$XDG_STATE_HOME/aizome/annai`, or `~/.local/state/aizome/annai`.
pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("aizome/annai")
}

pub fn path() -> PathBuf {
    dir().join("state.json")
}

/// The state at `path` (the default if it's missing or unreadable).
pub fn load(path: &Path) -> State {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Write `state` to `path`, through a temporary file.
pub fn save(path: &Path, state: &State) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, serde_json::to_string_pretty(state).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, path)
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_is_due_once_unless_asked_every_time() {
        let mut s = State::default();
        assert!(s.due());
        s.shown = Some(1);
        assert!(!s.due());
        s.show_at_login = true;
        assert!(s.due());
        let p = std::env::temp_dir().join(format!("annai-state-{}/state.json", std::process::id()));
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        assert_eq!(load(Path::new("/nonexistent/state.json")), State::default());
    }
}
