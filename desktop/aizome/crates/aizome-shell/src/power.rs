//! The power key, where the session handles it: `[power] power_key` says
//! what it does (ask, sleep, lock, shut down, nothing). While it's set,
//! the shell holds logind's `handle-power-key` block so logind leaves the
//! key alone; unset, logind does what `HandlePowerKey` says, and the shell
//! asks only where that's `ignore`. The compositor reports the key as the
//! `power-key` binding.

use aizome_config::PowerKey;
use aizome_kit::prelude::*;

use crate::actions::SessionVerb;
use crate::shell::{Msg, Shell};

impl Shell {
    /// What pressing the power key does now (`None`: logind's to handle).
    pub fn power_key_action(&self) -> Option<PowerKey> {
        match self.power_cfg.power_key {
            Some(k) => Some(k),
            None if self.logind_power_key.as_deref() == Some("ignore") => Some(PowerKey::Ask),
            None => None,
        }
    }

    /// The power key was pressed.
    pub fn power_key(&mut self, cx: &mut Cx<Msg>) {
        match self.power_key_action() {
            Some(PowerKey::Ask) => self.confirm_power(cx),
            Some(PowerKey::Sleep) => self.run(crate::actions::Action::Session(SessionVerb::Sleep), cx),
            Some(PowerKey::Lock) => self.lock(cx),
            Some(PowerKey::Off) => self.do_session(SessionVerb::ShutDown),
            Some(PowerKey::Nothing) | None => {}
        }
    }

    /// Hold logind's `handle-power-key` while `[power] power_key` is set,
    /// let it go when it isn't.
    pub fn sync_power_key(&mut self) {
        let want = self.power_cfg.power_key.is_some();
        if want == self.power_inhibit.is_some() {
            return;
        }
        if !want {
            self.power_inhibit = None;
            return;
        }
        if let Some(l) = &self.sys.logind {
            self.power_inhibit =
                Some(l.inhibit("handle-power-key", "Aizome's settings say what the power key does", "block"));
        }
    }

    /// Read logind's `HandlePowerKey`, on a thread.
    pub fn read_logind_power_key(&mut self, cx: &mut Cx<Msg>) {
        cx.spawn(|| Msg::LogindPowerKey(logind_handle_power_key()));
    }
}

/// logind's `HandlePowerKey` (`poweroff`, `suspend`, `ignore`…), from the
/// system bus.
pub fn logind_handle_power_key() -> Option<String> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .ok()?;
    proxy.get_property::<String>("HandlePowerKey").ok()
}
