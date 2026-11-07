//! The login screen's own compositor settings: the system's, with the last
//! person's public display scales (available before their home is
//! unlocked) and the keyboard a `keyboard_file` names. A private temporary
//! settings file keeps the system configuration intact and lives as long
//! as the greeter's compositor; setup rewrites its keyboard as the person
//! chooses one.

use std::path::{Path, PathBuf};

use aizome_config::{Settings, greeter::DisplayScales};

use crate::config::{Config, State};
use crate::users::User;

pub struct SettingsFile(PathBuf);

impl SettingsFile {
    pub fn path(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for SettingsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn user<'a>(state: &State, users: &'a [User]) -> Option<&'a User> {
    state
        .last_user
        .as_ref()
        .and_then(|name| users.iter().find(|u| &u.name == name))
        .or_else(|| if users.len() == 1 { users.first() } else { None })
}

/// Failure to read a preference must never prevent login.
pub fn prepare(cfg: &Config) -> Option<SettingsFile> {
    match prepare_inner(cfg) {
        Ok(settings) => settings,
        Err(e) => {
            log::warn!("the login screen's settings: {e}; using the compositor's defaults");
            None
        }
    }
}

/// The last person's display scales, if they saved any.
fn scales(cfg: &Config) -> Result<Vec<aizome_config::OutputConfig>, String> {
    let state = State::load(&cfg.state);
    let users = crate::users::find(&cfg.hidden_users);
    let Some(user) = user(&state, &users) else { return Ok(Vec::new()) };
    let bus = aizome_sys::Bus::system().map_err(|e| e.to_string())?;
    let text = aizome_sys::users::display_scales(&bus, user.uid).map_err(|e| e.to_string())?;
    Ok(DisplayScales::from_toml(&text).ok_or("invalid display scales")?.outputs)
}

/// The keyboard a `keyboard_file` gives, as far as it gives one.
#[derive(Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct KeyboardFile {
    pub layouts: Vec<String>,
    pub variant: Option<String>,
    pub options: Option<String>,
    pub model: Option<String>,
}

impl KeyboardFile {
    /// Read `path`, if it's root's: anyone else could choose the keyboard
    /// a password is typed on.
    pub fn read(path: &Path) -> Option<KeyboardFile> {
        let meta = std::fs::metadata(path).ok()?;
        if std::os::unix::fs::MetadataExt::uid(&meta) != 0 || !meta.is_file() || meta.len() > 64 * 1024 {
            return None;
        }
        toml::from_str(&std::fs::read_to_string(path).ok()?).ok().filter(|k: &KeyboardFile| !k.layouts.is_empty())
    }

    fn apply(self, k: &mut aizome_config::Keyboard) {
        k.layouts = self.layouts;
        k.variant = self.variant.unwrap_or_default();
        if let Some(o) = self.options {
            k.options = o;
        }
        if let Some(m) = self.model {
            k.model = m;
        }
    }
}

fn prepare_inner(cfg: &Config) -> Result<Option<SettingsFile>, String> {
    // The scales are a nicety: without them the keyboard still counts.
    let scales = scales(cfg).unwrap_or_else(|e| {
        log::warn!("login display scales: {e}; using the compositor's defaults");
        Vec::new()
    });
    let keyboard = cfg.keyboard_file.as_deref().and_then(KeyboardFile::read);
    // Setup changes the keyboard as it's shown: it needs a file of its own.
    if scales.is_empty() && keyboard.is_none() && cfg.setup_socket.is_none() {
        return Ok(None);
    }
    let path = cfg.compositor_settings.clone().unwrap_or_else(Settings::path);
    let mut settings = Settings::load_from(&path).map_err(|e| e.to_string())?;
    // Explicit system output rules take precedence. Everything else uses the
    // same display matching and scale as the person's Aizome session.
    settings.outputs.extend(scales);
    if let Some(k) = keyboard {
        k.apply(&mut settings.input.keyboard);
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR isn't set")?;
    let file = SettingsFile(PathBuf::from(runtime).join(format!("aizome-greeter-{}.toml", std::process::id())));
    settings.save_to(&file.0).map_err(|e| e.to_string())?;
    Ok(Some(file))
}

/// Type on `layout` at the login screen from now: the compositor's
/// settings file rewritten, and the compositor asked to read it again (it
/// would within a second anyway).
pub fn set_keyboard(path: &Path, layout: &str) -> Result<(), String> {
    let mut settings = Settings::load_from(path).map_err(|e| e.to_string())?;
    let k = &mut settings.input.keyboard;
    if k.layouts == [layout] && k.variant.is_empty() {
        return Ok(());
    }
    k.layouts = vec![layout.to_owned()];
    k.variant = String::new();
    settings.save_to(path).map_err(|e| e.to_string())?;
    if let Ok(mut c) = aizome_ipc::Client::connect() {
        let _ = c.request(&aizome_ipc::Request::ReloadConfig);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyboard_file_replaces_the_layout() {
        let k: KeyboardFile = toml::from_str("# setup\nlayouts = [\"de\"]\nvariant = \"\"\n").unwrap();
        let mut keyboard = aizome_config::Keyboard {
            layouts: vec!["us".into()],
            options: "compose:ralt".into(),
            ..Default::default()
        };
        k.apply(&mut keyboard);
        assert_eq!(keyboard.layouts, ["de"]);
        assert_eq!(keyboard.options, "compose:ralt", "options it doesn't name stay");
        assert!(toml::from_str::<KeyboardFile>("layouts = 3").is_err());
    }

    #[test]
    fn uses_the_last_person_or_the_only_person() {
        let alice = User { name: "alice".into(), uid: 1000, real_name: "Alice".into(), icon: None };
        let bob = User { name: "bob".into(), uid: 1001, real_name: "Bob".into(), icon: None };
        let users = [alice, bob];
        assert!(user(&State::default(), &users).is_none());
        assert_eq!(user(&State::default(), &users[..1]).unwrap().uid, 1000);
        let state = State { last_user: Some("bob".into()), ..Default::default() };
        assert_eq!(user(&state, &users).unwrap().uid, 1001);
        let state = State { last_user: Some("removed".into()), ..Default::default() };
        assert!(user(&state, &users).is_none());
        assert!(user(&state, &[]).is_none());
    }
}
