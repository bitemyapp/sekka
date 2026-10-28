//! Hotaru settings follow Aizome's per-application TOML convention.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use aizome_kit::menu::{Shortcut, ShortcutKey};
use aizome_kit::sidestep_ui::KeyEvent;
use serde::{Deserialize, Serialize};

use crate::app::Msg;

const LIMIT: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Desktop,
    Night,
    Day,
    HighContrast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Ignore,
    NewTab,
    ClosePane,
    CloseTab,
    SplitRight,
    SplitDown,
    PreviousPane,
    NextPane,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    Copy,
    Paste,
    CopyAll,
    Find,
    LargerText,
    SmallerText,
    ActualSize,
    ReloadConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub font_size: f64,
    pub font_weight: u16,
    pub theme: Theme,
    pub scrollback: usize,
    pub keybindings: BTreeMap<String, Action>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            font_weight: 400,
            theme: Theme::Desktop,
            scrollback: 10_000,
            keybindings: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bindings {
    bindings: Vec<(Shortcut, Action)>,
    custom: Vec<(Shortcut, Action)>,
}

pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").filter(|path| !path.is_empty()).map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|directory| directory.join("aizome/hotaru.toml"))
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let mut data = String::new();
        if !file.metadata().map_err(|error| error.to_string())?.is_file() {
            return Err("Configuration must be a regular file.".into());
        }
        file.take(LIMIT + 1).read_to_string(&mut data).map_err(|error| error.to_string())?;
        if data.len() as u64 > LIMIT {
            return Err("Configuration exceeds 64 KiB.".into());
        }
        Self::parse(&data)
    }

    pub fn parse(data: &str) -> Result<Self, String> {
        let settings: Self = toml::from_str(data).map_err(|error| error.to_string())?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.font_size.is_finite() || !(8.0..=72.0).contains(&self.font_size) {
            return Err("font_size must be between 8 and 72 points.".into());
        }
        if !(100..=900).contains(&self.font_weight) {
            return Err("font_weight must be between 100 and 900.".into());
        }
        if self.scrollback > 1_000_000 {
            return Err("scrollback cannot exceed 1,000,000 rows.".into());
        }
        if self.keybindings.len() > 128 {
            return Err("At most 128 custom keybindings are supported.".into());
        }
        for key in self.keybindings.keys() {
            parse_shortcut(key)?;
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        self.validate()?;
        let data = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        let directory = path.parent().ok_or("Configuration has no parent directory.")?;
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err("Configuration is a symbolic link; edit its source instead.".into());
        }
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = directory.join(format!(".hotaru-{}-{next}.tmp", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let mut operation = || -> std::io::Result<()> {
            if let Ok(metadata) = std::fs::metadata(path) {
                file.set_permissions(std::fs::Permissions::from_mode(metadata.permissions().mode() & 0o777))?;
            }
            file.write_all(data.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)?;
            Ok(())
        };
        let result = operation().map_err(|error| error.to_string());
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

pub fn parse_shortcut(value: &str) -> Result<Shortcut, String> {
    if value.len() > 80 {
        return Err("Keybinding is too long.".into());
    }
    let mut parts = value.split('+').map(str::trim).peekable();
    let mut shortcut = Shortcut::key(ShortcutKey::Escape);
    while let Some(part) = parts.next() {
        let part = part.to_ascii_lowercase();
        if parts.peek().is_some() {
            match part.as_str() {
                "super" | "cmd" => shortcut.command = true,
                "ctrl" | "control" => shortcut.ctrl = true,
                "alt" => shortcut.alt = true,
                "shift" => shortcut.shift = true,
                _ => return Err(format!("Unknown key modifier in {value:?}.")),
            }
        } else {
            shortcut.key = match part.as_str() {
                "enter" | "return" => ShortcutKey::Enter,
                "tab" => ShortcutKey::Tab,
                "space" => ShortcutKey::Space,
                "escape" | "esc" => ShortcutKey::Escape,
                "backspace" => ShortcutKey::Backspace,
                "delete" => ShortcutKey::Delete,
                "left" => ShortcutKey::Left,
                "right" => ShortcutKey::Right,
                "up" => ShortcutKey::Up,
                "down" => ShortcutKey::Down,
                "home" => ShortcutKey::Home,
                "end" => ShortcutKey::End,
                "pageup" => ShortcutKey::PageUp,
                "pagedown" => ShortcutKey::PageDown,
                "plus" => ShortcutKey::Char('+'),
                _ if part.starts_with('f') && part[1..].parse::<u8>().is_ok_and(|n| (1..=35).contains(&n)) => {
                    ShortcutKey::F(part[1..].parse().unwrap())
                }
                _ if part.chars().count() == 1 => ShortcutKey::Char(part.chars().next().unwrap()),
                _ => return Err(format!("Unknown key in {value:?}.")),
            };
        }
    }
    Ok(shortcut)
}

fn defaults() -> Vec<(Shortcut, Action)> {
    use Action::*;
    [
        ("super+t", NewTab),
        ("super+w", ClosePane),
        ("super+alt+w", CloseTab),
        ("super+d", SplitRight),
        ("super+shift+d", SplitDown),
        ("super+[", PreviousPane),
        ("super+]", NextPane),
        ("super+alt+left", FocusLeft),
        ("super+alt+right", FocusRight),
        ("super+alt+up", FocusUp),
        ("super+alt+down", FocusDown),
        ("super+c", Copy),
        ("super+v", Paste),
        ("super+shift+c", CopyAll),
        ("super+f", Find),
        ("super+plus", LargerText),
        ("super+-", SmallerText),
        ("super+0", ActualSize),
        ("super+shift+,", ReloadConfig),
        // Familiar Linux terminal aliases coexist with Aizome's command key.
        ("ctrl+shift+c", Copy),
        ("ctrl+shift+v", Paste),
        ("ctrl+shift+o", SplitRight),
        ("ctrl+shift+e", SplitDown),
        ("ctrl+shift+t", NewTab),
        ("ctrl+shift+w", ClosePane),
        ("ctrl+alt+left", FocusLeft),
        ("ctrl+alt+right", FocusRight),
        ("ctrl+alt+up", FocusUp),
        ("ctrl+alt+down", FocusDown),
    ]
    .into_iter()
    .map(|(key, action)| (parse_shortcut(key).unwrap(), action))
    .collect()
}

impl Bindings {
    pub fn new(settings: &Settings) -> Self {
        let custom: Vec<_> = settings
            .keybindings
            .iter()
            .filter_map(|(key, action)| parse_shortcut(key).ok().map(|key| (key, *action)))
            .collect();
        let mut bindings = defaults();
        for &(key, action) in &custom {
            bindings.retain(|(existing, _)| *existing != key);
            bindings.push((key, action));
        }
        Self { bindings, custom }
    }

    pub fn action(&self, key: &KeyEvent) -> Option<Action> {
        self.bindings.iter().rev().find(|(shortcut, _)| shortcut.matches(key)).map(|(_, action)| *action)
    }

    pub fn shortcut(&self, action: Action) -> Option<Shortcut> {
        self.custom
            .iter()
            .rev()
            .chain(self.bindings.iter())
            .find(|(_, candidate)| *candidate == action)
            .map(|(key, _)| *key)
    }
}

enum Job {
    Load(u64),
    Save(u64, Settings),
}
#[derive(Default)]
struct Queue {
    job: Option<Job>,
    stop: bool,
}
pub struct Worker {
    shared: Arc<(Mutex<Queue>, Condvar)>,
}

impl Worker {
    pub fn new(path: PathBuf, sender: aizome_kit::MsgSender<Msg>) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let worker = shared.clone();
        std::thread::Builder::new().name("hotaru-settings".into()).spawn(move || {
            loop {
                let (lock, changed) = &*worker;
                let mut queue = lock.lock().unwrap_or_else(|error| error.into_inner());
                while queue.job.is_none() && !queue.stop {
                    queue = changed.wait(queue).unwrap_or_else(|error| error.into_inner());
                }
                if queue.stop && queue.job.is_none() {
                    break;
                }
                let job = queue.job.take().unwrap();
                drop(queue);
                match job {
                    Job::Load(token) => {
                        sender.send(Msg::ConfigLoaded(token, Settings::load(&path)));
                    }
                    Job::Save(token, settings) => {
                        sender.send(Msg::ConfigSaved(token, settings.save(&path)));
                    }
                }
            }
        })?;
        Ok(Self { shared })
    }
    fn submit(&self, job: Job) {
        let (lock, changed) = &*self.shared;
        lock.lock().unwrap_or_else(|error| error.into_inner()).job = Some(job);
        changed.notify_one();
    }
    pub fn load(&self, token: u64) {
        self.submit(Job::Load(token));
    }
    pub fn save(&self, token: u64, settings: Settings) {
        self.submit(Job::Save(token, settings));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, changed) = &*self.shared;
        lock.lock().unwrap_or_else(|error| error.into_inner()).stop = true;
        changed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_settings_and_typographical_errors() {
        for text in [
            "font_size = nan",
            "font_size = 999",
            "font_sizee = 14",
            "font_weight = 9",
            "[keybindings]\n'ctrl+wrong+q' = 'copy'",
        ] {
            assert!(Settings::parse(text).is_err(), "{text}");
        }
    }
    #[test]
    fn configured_binding_replaces_a_default_without_stealing_control_c() {
        let settings = Settings::parse("[keybindings]\n'super+d' = 'close-pane'\n'ctrl+shift+o' = 'ignore'").unwrap();
        let bindings = Bindings::new(&settings);
        let key = |key: &str, modifiers| KeyEvent {
            key: aizome_kit::sidestep_ui::Key::Character(key.into()),
            text: None,
            pressed: true,
            repeat: false,
            code: 0,
            modifiers,
            composing: None,
            native: Default::default(),
        };
        use aizome_kit::sidestep_ui::Modifiers;
        assert_eq!(bindings.action(&key("d", Modifiers::SUPER)), Some(Action::ClosePane));
        assert_eq!(bindings.action(&key("o", Modifiers::CONTROL | Modifiers::SHIFT)), Some(Action::Ignore));
        assert_eq!(bindings.action(&key("c", Modifiers::CONTROL)), None);
        assert_eq!(Settings::parse(&toml::to_string(&settings).unwrap()).unwrap(), settings);
    }
    #[test]
    fn saving_is_atomic_and_preserves_managed_symlinks() {
        let directory = std::env::temp_dir().join(format!(
            "hotaru-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("hotaru.toml");
        let settings = Settings { font_size: 19.0, theme: Theme::Day, ..Settings::default() };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        let managed = directory.join("managed.toml");
        std::os::unix::fs::symlink(&path, &managed).unwrap();
        assert!(Settings::default().save(&managed).is_err());
        assert!(std::fs::symlink_metadata(&managed).unwrap().file_type().is_symlink());
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
