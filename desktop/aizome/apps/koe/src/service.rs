//! Whether dictation runs, and the shortcuts that work it.
//!
//! Koe's window (or `aizome-koe enable`) keeps the choice as koe.toml's
//! `enabled` and starts the session's service at once; at each login the
//! session starts it again while it's on (the unit's `ExecCondition` is
//! `aizome-koe enabled`). The shortcuts are the compositor's, in
//! settings.toml's `[keys.bindings]`.
use crate::{
    config::Config,
    ipc,
    protocol::{Action, Request},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const UNIT: &str = "aizome-koe.service";

/// Starts and stops the dictation service. Both block until it's done.
pub trait Service: Send + Sync {
    fn start(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
}

/// The session's service: its systemd user unit where it's the default
/// configuration and socket, else (or with no unit: a build run from a
/// checkout) a daemon of this binary's own.
pub struct Session {
    pub config: PathBuf,
    pub socket: PathBuf,
    pub unit: bool,
}

impl Session {
    pub fn user() -> Result<Session> {
        Ok(Session { config: crate::config::config_path()?, socket: ipc::socket_path()?, unit: true })
    }
}

fn systemctl(verb: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", verb, UNIT])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether a daemon answers on `socket`.
pub fn running(socket: &Path) -> bool {
    ipc::request_blocking(socket, &Request::new(Action::Status), Duration::from_secs(2)).is_ok()
}

fn wait(socket: &Path, up: bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while running(socket) != up {
        if Instant::now() > deadline {
            bail!(if up { "Koe's service didn't start" } else { "Koe's service didn't stop" });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// The unit's own last fatal error since `since`, from the journal, so the
/// window can say why it didn't start.
fn why(since: SystemTime) -> Option<String> {
    let since = since.duration_since(UNIX_EPOCH).ok()?.as_secs().saturating_sub(1);
    let out = Command::new("journalctl")
        .args(["--user", "--no-pager", "--output=cat", "--lines=50"])
        .arg(format!("--since=@{since}"))
        .arg(format!("_SYSTEMD_USER_UNIT={UNIT}"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    reason(&String::from_utf8_lossy(&out.stdout)).map(str::to_owned)
}

/// The last fatal error in a daemon's output (main prints them as one line).
fn reason(output: &str) -> Option<&str> {
    output.lines().rev().find_map(|line| line.strip_prefix("aizome-koe: ")).map(str::trim).filter(|r| !r.is_empty())
}

impl Service for Session {
    fn start(&self) -> Result<()> {
        if running(&self.socket) {
            return Ok(());
        }
        let asked = SystemTime::now();
        if self.unit && systemctl("start") {
            return wait(&self.socket, true).map_err(|error| match why(asked) {
                Some(reason) => anyhow::anyhow!("Koe's service stopped: {reason}"),
                None => error,
            });
        }
        use std::os::unix::process::CommandExt;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--config")
            .arg(&self.config)
            .arg("--socket")
            .arg(&self.socket)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Not in this program's process group: it outlives a terminal.
            .process_group(0)
            .spawn()
            .context("cannot start Koe's daemon")?;
        if let Err(error) = wait(&self.socket, true) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        std::thread::spawn(move || child.wait());
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        if self.unit {
            systemctl("stop");
        }
        // One of our own, or a unit that wasn't the one running.
        if ipc::request_blocking(&self.socket, &Request::new(Action::Shutdown), Duration::from_secs(2)).is_ok() {
            wait(&self.socket, false)?;
        }
        Ok(())
    }
}

/// Whether dictation is on (a configuration that can't be read is off).
pub fn enabled(config: &Path) -> bool {
    Config::load(config).is_ok_and(|c| c.enabled)
}

/// Turn dictation on or off in the configuration, keeping the rest.
pub fn set_enabled(path: &Path, on: bool) -> Result<Config> {
    let mut config = Config::load(path).with_context(|| format!("fix or remove {} first", path.display()))?;
    config.enabled = on;
    config.save(path)?;
    Ok(config)
}

/// The compositor's action for starting or stopping dictation.
pub const TOGGLE: &str = "spawn aizome-koe record toggle";
pub const CANCEL: &str = "spawn aizome-koe record cancel";
/// Koe before it was one of Aizome's apps ran as `koe`.
const LEGACY: &str = "spawn koe record ";
/// The shortcuts dictation is given when it has none: ❖⌥D and ❖⌥⎋.
pub const DEFAULTS: [(&str, &str); 2] = [("Super+Alt+D", TOGGLE), ("Super+Alt+Escape", CANCEL)];

/// The chords bound to `action`, as settings.toml writes them.
pub fn chords(bindings: &BTreeMap<String, String>, action: &str) -> Vec<String> {
    bindings.iter().filter(|(_, a)| a.as_str() == action).map(|(c, _)| c.clone()).collect()
}

/// A chord's modifiers and key, however it's written.
fn parts(chord: &str) -> ([bool; 4], String) {
    let mut mods = [false; 4];
    let mut key = String::new();
    for part in chord.split('+').map(str::trim) {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" | "⌃" => mods[0] = true,
            "alt" | "option" | "opt" | "⌥" => mods[1] = true,
            "shift" | "⇧" => mods[2] = true,
            "super" | "logo" | "cmd" | "mod4" | "win" | "❖" => mods[3] = true,
            "esc" => key = "escape".into(),
            k => key = k.into(),
        }
    }
    (mods, key)
}

/// A chord as keycaps, in Aizome's order: ⌃ ⌥ ⇧ ❖ and the key.
pub fn keycaps(chord: &str) -> Vec<String> {
    let (mods, key) = parts(chord);
    let mut caps: Vec<String> =
        ["⌃", "⌥", "⇧", "❖"].iter().zip(mods).filter(|(_, on)| *on).map(|(g, _)| (*g).to_owned()).collect();
    caps.push(match key.as_str() {
        "escape" | "esc" => "⎋".into(),
        "return" | "enter" => "↩".into(),
        "space" => "Space".into(),
        "backspace" => "⌫".into(),
        "tab" => "⇥".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => {
            let mut c = k.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    });
    caps
}

/// Give dictation its shortcuts where it has none, ❖⌥D and ❖⌥⎋, unless
/// another action has them; and move Koe's old `spawn koe record …`
/// bindings to this binary. Whether anything changed.
pub fn add_shortcuts(bindings: &mut BTreeMap<String, String>) -> bool {
    let mut changed = false;
    for action in bindings.values_mut() {
        if let Some(rest) = action.strip_prefix(LEGACY) {
            *action = format!("spawn aizome-koe record {rest}");
            changed = true;
        }
    }
    for (chord, action) in DEFAULTS {
        let taken = bindings.keys().any(|c| parts(c) == parts(chord));
        if chords(bindings, action).is_empty() && !taken {
            bindings.insert(chord.into(), action.into());
            changed = true;
        }
    }
    changed
}

/// Add dictation's shortcuts to the settings file at `path`, and ask the
/// compositor to read it again when `tell`. The toggle's chords after.
pub fn ensure_shortcuts(path: &Path, tell: bool) -> Result<Vec<String>> {
    let mut settings =
        aizome_config::Settings::load_from(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    if add_shortcuts(&mut settings.keys.bindings) {
        settings.save_to(path).map_err(|e| anyhow::anyhow!("cannot save {}: {e}", path.display()))?;
        if tell && aizome_ipc::socket_path().is_some() {
            // The compositor also notices the file within a second.
            let _ = aizome_ipc::Client::connect().and_then(|mut c| c.request(&aizome_ipc::Request::ReloadConfig));
        }
    }
    Ok(chords(&settings.keys.bindings, TOGGLE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_start_says_the_daemons_own_last_error() {
        let journal = "aizome-koe: cannot bind the socket: first\n\
                       [2026-10-24T20:00:00Z WARN  koe::daemon] something\n\
                       aizome-koe: koe.toml: engine.threads must be at least 1\n\
                       \n";
        assert_eq!(reason(journal), Some("koe.toml: engine.threads must be at least 1"));
        assert_eq!(reason("Started aizome-koe.service.\n"), None);
        assert_eq!(reason(""), None);
    }

    #[test]
    fn shortcuts_are_added_once_never_over_others_and_old_ones_move() {
        let mut b = BTreeMap::new();
        assert!(add_shortcuts(&mut b));
        assert_eq!(chords(&b, TOGGLE), ["Super+Alt+D"]);
        assert_eq!(chords(&b, CANCEL), ["Super+Alt+Escape"]);
        assert!(!add_shortcuts(&mut b));

        // Someone's own chord stays; a chord another action has is left.
        let mut b = BTreeMap::from([
            ("Ctrl+Super+D".to_owned(), TOGGLE.to_owned()),
            ("alt+super+escape".to_owned(), "spawn foot".to_owned()),
        ]);
        assert!(!add_shortcuts(&mut b));
        assert_eq!(chords(&b, TOGGLE), ["Ctrl+Super+D"]);
        assert!(chords(&b, CANCEL).is_empty());

        let mut b = BTreeMap::from([("Super+Alt+D".to_owned(), "spawn koe record toggle".to_owned())]);
        assert!(add_shortcuts(&mut b));
        assert_eq!(chords(&b, TOGGLE), ["Super+Alt+D"]);
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn shortcut_aliases_keep_the_actions_already_bound_to_them() {
        for chord in ["Super+Alt+Esc", "Option+Cmd+Esc", "⌥+❖+Escape", "❖+⌥+Esc"] {
            let mut bindings = BTreeMap::from([
                ("Super+Alt+D".to_owned(), TOGGLE.to_owned()),
                (chord.to_owned(), "spawn foot".to_owned()),
            ]);
            assert!(!add_shortcuts(&mut bindings), "rebound {chord}");
            assert_eq!(bindings[chord], "spawn foot");
            assert!(chords(&bindings, CANCEL).is_empty());
        }
        for chord in ["⌥+❖+D", "❖+Alt+d", "Super+⌥+D"] {
            let mut bindings = BTreeMap::from([
                ("Super+Alt+Escape".to_owned(), CANCEL.to_owned()),
                (chord.to_owned(), "spawn foot".to_owned()),
            ]);
            assert!(!add_shortcuts(&mut bindings), "rebound {chord}");
            assert_eq!(bindings[chord], "spawn foot");
            assert!(chords(&bindings, TOGGLE).is_empty());
        }
    }

    #[test]
    fn keycaps_follow_aizome_order() {
        assert_eq!(keycaps("Super+Alt+D"), ["⌥", "❖", "D"]);
        assert_eq!(keycaps("Super+Alt+Escape"), ["⌥", "❖", "⎋"]);
        assert_eq!(keycaps("ctrl+shift+super+F5"), ["⌃", "⇧", "❖", "F5"]);
    }
}
