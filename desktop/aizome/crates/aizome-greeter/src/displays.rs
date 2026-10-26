//! The last person's public display scales, available before their home is
//! unlocked. A private temporary settings file keeps the system configuration
//! intact and lives as long as the greeter's compositor.

use std::path::PathBuf;

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
            log::warn!("login display scales: {e}; using the compositor's defaults");
            None
        }
    }
}

fn prepare_inner(cfg: &Config) -> Result<Option<SettingsFile>, String> {
    let state = State::load(&cfg.state);
    let users = crate::users::find(&cfg.hidden_users);
    let Some(user) = user(&state, &users) else { return Ok(None) };
    let bus = aizome_sys::Bus::system().map_err(|e| e.to_string())?;
    let text = aizome_sys::users::display_scales(&bus, user.uid).map_err(|e| e.to_string())?;
    let scales = DisplayScales::from_toml(&text).ok_or("invalid display scales")?;
    if scales.outputs.is_empty() {
        return Ok(None);
    }
    let path = cfg.compositor_settings.clone().unwrap_or_else(Settings::path);
    let mut settings = Settings::load_from(&path).map_err(|e| e.to_string())?;
    // Explicit system output rules take precedence. Everything else uses the
    // same display matching and scale as the person's Aizome session.
    settings.outputs.extend(scales.outputs);
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR isn't set")?;
    let file = SettingsFile(PathBuf::from(runtime).join(format!("aizome-greeter-{}.toml", std::process::id())));
    settings.save_to(&file.0).map_err(|e| e.to_string())?;
    Ok(Some(file))
}

#[cfg(test)]
mod tests {
    use super::*;

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
