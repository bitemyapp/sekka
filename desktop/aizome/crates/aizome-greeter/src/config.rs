//! The greeter's configuration, `/etc/aizome/greeter.toml` (Aizome's NixOS
//! module writes it; `docs/greeter.md`), and what it remembers between
//! logins, `/var/lib/aizome-greeter/state.toml`: who logged in last and
//! each person's last session. Nothing secret is in either.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG: &str = "/etc/aizome/greeter.toml";

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Session directories, in order of preference.
    pub sessions: Vec<PathBuf>,
    /// Session ids not to offer.
    pub hidden_sessions: Vec<String>,
    /// What's picked for someone who hasn't logged in here before.
    pub default_session: Option<String>,
    /// How an X11 session starts (`["startx", "/usr/bin/env"]`); empty, and
    /// X11 sessions aren't offered.
    pub x11_prefix: Vec<String>,
    /// Accounts not to list.
    pub hidden_users: Vec<String>,
    /// List people to pick from; else the user name is typed.
    pub user_list: bool,
    /// The wallpapers, by night and by day.
    pub wallpaper_night: Option<PathBuf>,
    pub wallpaper_day: Option<PathBuf>,
    /// `night`, `day`, or `auto` (day from 06:00 to 18:00).
    pub wallpaper: String,
    /// Offer Sleep, Restart and Shut Down.
    pub power: bool,
    /// The compositor and its arguments (the greeter adds `--socket`).
    pub compositor: Vec<String>,
    /// The compositor's settings (its keyboard layout, the displays).
    pub compositor_settings: Option<PathBuf>,
    pub state: PathBuf,
    /// The computer's name, shown at the top left (else the host name).
    pub hostname: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            sessions: vec![
                "/run/current-system/sw/share/wayland-sessions".into(),
                "/run/current-system/sw/share/xsessions".into(),
            ],
            hidden_sessions: Vec::new(),
            default_session: None,
            x11_prefix: Vec::new(),
            hidden_users: vec!["nobody".into()],
            user_list: true,
            wallpaper_night: None,
            wallpaper_day: None,
            wallpaper: "auto".into(),
            power: true,
            compositor: vec![
                "aizome".into(),
                "--backend".into(),
                "udev".into(),
                "--no-shell".into(),
                "--no-xwayland".into(),
                "--no-session".into(),
            ],
            compositor_settings: None,
            state: "/var/lib/aizome-greeter/state.toml".into(),
            hostname: None,
        }
    }
}

impl Config {
    /// Read `path`; the defaults where it's missing. A broken file is an
    /// error: better to say so than to come up half-configured.
    pub fn load(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// What's remembered.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub last_user: Option<String>,
    /// Each person's last session.
    pub sessions: BTreeMap<String, String>,
}

impl State {
    /// Read it; empty if it isn't there or doesn't read.
    pub fn load(path: &Path) -> State {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    /// Write it whole: a new file, renamed over the old.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        let new = path.with_extension("new");
        std::fs::write(&new, text)?;
        std::fs::rename(new, path)
    }

    /// `user` logged in to `session`.
    pub fn logged_in(&mut self, user: &str, session: &str) {
        self.last_user = Some(user.to_owned());
        self.sessions.insert(user.to_owned(), session.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_reads_with_defaults_for_the_rest() {
        let c: Config = toml::from_str(
            r#"
sessions = ["/s/wayland-sessions", "/s/xsessions"]
default_session = "aizome"
x11_prefix = ["startx", "/usr/bin/env"]
"#,
        )
        .unwrap();
        assert_eq!(c.default_session.as_deref(), Some("aizome"));
        assert_eq!(c.sessions.len(), 2);
        assert!(c.power && c.user_list);
        assert_eq!(c.compositor[0], "aizome");
        // A misspelt key is an error, not silently ignored.
        assert!(toml::from_str::<Config>("defualt_session = \"x\"").is_err());
        assert_eq!(Config::load(Path::new("/nonexistent/greeter.toml")).unwrap(), Config::default());
    }

    #[test]
    fn the_state_remembers_who_and_what() {
        let path = std::env::temp_dir().join(format!("aizome-greeter-state-{}.toml", std::process::id()));
        let mut s = State::load(&path);
        assert_eq!(s, State::default());
        s.logged_in("alice", "aizome");
        s.logged_in("bob", "tatami");
        s.logged_in("alice", "plasma");
        s.save(&path).unwrap();
        let back = State::load(&path);
        assert_eq!(back.last_user.as_deref(), Some("alice"));
        assert_eq!(back.sessions.get("alice").map(String::as_str), Some("plasma"));
        assert_eq!(back.sessions.get("bob").map(String::as_str), Some("tatami"));
        std::fs::remove_file(&path).unwrap();
    }
}
