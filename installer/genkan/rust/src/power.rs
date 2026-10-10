// SPDX-License-Identifier: GPL-3.0-or-later
//! Keeps the machine awake while installing.
//!
//! A laptop that sleeps during installation is left with a half-written
//! disk, and some graphics drivers do not wake a live system at all. While
//! installing, the helper holds a logind block on sleep and idle: the
//! desktop cannot suspend the machine (idle timer, lid, Sleep button), and
//! Plasma mirrors the lock, so dimming, screen-off and locking wait too. The
//! kernel releases it when the descriptor closes, also if the helper dies.
use std::os::fd::OwnedFd;

/// Held for as long as the machine must stay awake.
pub struct Awake {
    lock: Option<OwnedFd>,
}

impl Awake {
    /// Whether logind granted the lock.
    pub fn held(&self) -> bool {
        self.lock.is_some()
    }
}

/// Block sleep and idle until the guard is dropped. Without logind (a
/// container or a build sandbox) installation proceeds unprotected.
pub fn stay_awake(why: &str) -> Awake {
    let lock = inhibit(why)
        .inspect_err(|error| eprintln!("Not keeping the machine awake: {error}"))
        .ok();
    Awake { lock }
}

#[cfg(feature = "network")]
fn inhibit(why: &str) -> Result<OwnedFd, String> {
    use gio::prelude::*;
    let bus = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)
        .map_err(|error| error.to_string())?;
    let arguments = ("sleep:idle", "NixOS installer", why, "block").to_variant();
    let (reply, fds) = bus
        .call_with_unix_fd_list_sync(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "Inhibit",
            Some(&arguments),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            None::<&gio::UnixFDList>,
            gio::Cancellable::NONE,
        )
        .map_err(|error| error.to_string())?;
    let (handle,) = reply
        .get::<(glib::variant::Handle,)>()
        .ok_or("logind returned no lock")?;
    fds.ok_or("logind returned no lock")?
        .get(handle.0)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "network"))]
fn inhibit(_: &str) -> Result<OwnedFd, String> {
    Err("built without D-Bus support".into())
}
