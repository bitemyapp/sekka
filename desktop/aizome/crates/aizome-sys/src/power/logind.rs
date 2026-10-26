//! The session and the machine's power, through systemd-logind: suspend,
//! hibernate, reboot and power off (with the `Can…` answers that say
//! whether to offer them), the session's lock and unlock requests,
//! `PrepareForSleep` with a delay inhibitor so the shell can lock the screen
//! before the machine sleeps, the lid, the idle hint, and inhibitors.
//!
//! # Locking before sleep
//!
//! ```ignore
//! let logind = Logind::new(wake);
//! logind.take_sleep_delay()?;               // once, at start
//! // When the wake fires:
//! for event in logind.take_events() {
//!     match event {
//!         SessionEvent::PrepareForSleep => { lock_screen(); logind.sleep_ready(); }
//!         SessionEvent::Lock => lock_screen(),
//!         _ => {}
//!     }
//! }
//! ```
//!
//! logind waits for the delay inhibitor to be released (at most
//! `InhibitDelayMaxSec`, 5 seconds by default) before sleeping; after
//! resuming, the inhibitor is taken again by itself.

use crate::bus::{self, Bus, Event, Opts, Props, PropsExt, signals_from};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, lock, service_handle};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::zvariant::{OwnedFd, OwnedObjectPath};

pub(crate) const LOGIN1: &str = "org.freedesktop.login1";
pub(crate) const MANAGER_PATH: &str = "/org/freedesktop/login1";
pub(crate) const IFACE_MANAGER: &str = "org.freedesktop.login1.Manager";
pub(crate) const IFACE_SESSION: &str = "org.freedesktop.login1.Session";

/// Whether an action is allowed (logind's `Can…` answers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Can {
    /// Allowed.
    Yes,
    /// Allowed after authenticating (polkit will ask).
    Challenge,
    /// Not allowed.
    No,
    /// Possible, but something holds a block inhibitor for it now (an app
    /// keeping the machine awake). Asking anyway overrides the inhibitor,
    /// which polkit may want a password for.
    Inhibited,
    /// Not supported on this machine (no swap to hibernate to).
    #[default]
    NotApplicable,
}

impl Can {
    pub fn parse(s: &str) -> Can {
        match s {
            "yes" => Can::Yes,
            "challenge" => Can::Challenge,
            "no" => Can::No,
            "inhibited" => Can::Inhibited,
            _ => Can::NotApplicable,
        }
    }

    /// Whether to offer the action.
    pub fn offered(self) -> bool {
        matches!(self, Can::Yes | Can::Challenge | Can::Inhibited)
    }
}

/// What logind reports for the machine and this session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogindState {
    pub available: bool,
    /// This session's id and object path.
    pub session_id: String,
    pub session_path: String,
    pub can_power_off: Can,
    pub can_reboot: Can,
    pub can_suspend: Can,
    pub can_hibernate: Can,
    pub can_hybrid_sleep: Can,
    pub can_suspend_then_hibernate: Can,
    pub lid_closed: bool,
    pub docked: bool,
    pub on_external_power: bool,
    /// All sessions are idle.
    pub idle_hint: bool,
    pub idle_since: Option<SystemTime>,
    /// The machine is about to sleep or shut down.
    pub preparing_for_sleep: bool,
    pub preparing_for_shutdown: bool,
    /// This session is the active one on its seat.
    pub session_active: bool,
    pub session_locked_hint: bool,
    pub session_idle_hint: bool,
    /// What is blocked or delayed right now (`sleep:idle:handle-lid-switch`).
    pub block_inhibited: String,
    pub delay_inhibited: String,
    /// We hold a sleep delay ([`Logind::take_sleep_delay`]).
    pub sleep_delay_held: bool,
}

/// Something that happened to the session, in order. Read them with
/// [`Logind::take_events`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionEvent {
    /// Lock the screen (`loginctl lock-session`, or before sleep).
    Lock,
    /// Unlock the screen (`loginctl unlock-session`).
    Unlock,
    /// The machine is about to sleep: lock, then call
    /// [`Logind::sleep_ready`].
    PrepareForSleep,
    /// The machine woke up.
    Resumed,
    /// The machine is about to power off or reboot.
    PrepareForShutdown,
}

/// A held inhibitor (`systemd-inhibit`): released when dropped.
#[derive(Debug)]
pub struct Inhibition {
    _fd: OwnedFd,
}

/// An inhibitor someone holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inhibitor {
    /// `sleep`, `shutdown`, `idle`, `handle-lid-switch`…, colon-separated.
    pub what: String,
    pub who: String,
    pub why: String,
    /// `block`, `block-weak` or `delay`.
    pub mode: String,
    pub uid: u32,
    pub pid: u32,
}

#[derive(Default)]
struct Shared {
    events: Vec<SessionEvent>,
    /// The sleep delay inhibitor, while held.
    delay: Option<OwnedFd>,
    /// Take the delay again after each resume.
    want_delay: bool,
    session_path: Option<String>,
}

/// systemd-logind.
pub struct Logind {
    bus: Option<Bus>,
    model: Arc<Model<LogindState>>,
    shared: Arc<Mutex<Shared>>,
    stop: Stop,
}

service_handle!(Logind, LogindState);

const WHO: &str = "Aizome";

impl Logind {
    /// Follow logind for this process's session.
    pub fn new(wake: Wake) -> Logind {
        match Bus::system() {
            Ok(bus) => Logind::with_bus(bus, wake, None),
            Err(_) => Logind {
                bus: None,
                model: Arc::new(Model::new(LogindState::default(), wake)),
                shared: Arc::default(),
                stop: Stop::default(),
            },
        }
    }

    /// Follow logind on `bus`, for the session with id `session` (by
    /// default `$XDG_SESSION_ID`, else this process's session, else the
    /// user's graphical session).
    pub fn with_bus(bus: Bus, wake: Wake, session: Option<String>) -> Logind {
        let model = Arc::new(Model::new(LogindState::default(), wake));
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let stop = Stop::default();
        let follower = model.clone();
        let fshared = shared.clone();
        let mut manager = Props::new();
        let mut session_props = Props::new();
        let mut state = LogindState::default();
        let mut present = false;
        let mut dirty = false;
        bus::follow(bus.clone(), LOGIN1, vec![signals_from(LOGIN1)], false, stop.clone(), move |bus, event| {
            match event {
                Event::Appeared => {
                    present = true;
                    manager = bus.get_all(LOGIN1, MANAGER_PATH, IFACE_MANAGER).unwrap_or_default();
                    let (id, path) = find_session(bus, session.as_deref()).unwrap_or_default();
                    session_props = if path.is_empty() {
                        Props::new()
                    } else {
                        bus.get_all(LOGIN1, &path, IFACE_SESSION).unwrap_or_default()
                    };
                    lock(&fshared).session_path = Some(path.clone()).filter(|p| !p.is_empty());
                    let can = |method: &str| {
                        Can::parse(
                            &bus.call::<_, String>(LOGIN1, MANAGER_PATH, IFACE_MANAGER, method, &())
                                .unwrap_or_default(),
                        )
                    };
                    state = LogindState {
                        available: true,
                        session_id: id,
                        session_path: path,
                        can_power_off: can("CanPowerOff"),
                        can_reboot: can("CanReboot"),
                        can_suspend: can("CanSuspend"),
                        can_hibernate: can("CanHibernate"),
                        can_hybrid_sleep: can("CanHybridSleep"),
                        can_suspend_then_hibernate: can("CanSuspendThenHibernate"),
                        ..LogindState::default()
                    };
                    // A restarted logind forgot our inhibitor.
                    let mut shared = lock(&fshared);
                    if shared.want_delay {
                        shared.delay = take_delay(bus).ok();
                    }
                    dirty = true;
                }
                Event::Vanished => {
                    present = false;
                    lock(&fshared).delay = None;
                    follower.set_if_changed(LogindState::default());
                }
                Event::Signal(msg) => {
                    let (iface, member) = bus::member(&msg);
                    let path = bus::path(&msg);
                    match (iface.as_str(), member.as_str()) {
                        ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                            if let Ok((changed_iface, changed, invalidated)) =
                                msg.body().deserialize::<(String, Props, Vec<String>)>()
                            {
                                if changed_iface == IFACE_MANAGER && path == MANAGER_PATH {
                                    manager.extend(changed);
                                    for name in invalidated {
                                        if let Ok(value) = bus.get(LOGIN1, MANAGER_PATH, IFACE_MANAGER, &name) {
                                            manager.insert(name, value);
                                        }
                                    }
                                    dirty = true;
                                } else if changed_iface == IFACE_SESSION && path == state.session_path {
                                    session_props.extend(changed);
                                    dirty = true;
                                }
                            }
                        }
                        (IFACE_SESSION, "Lock") if path == state.session_path => {
                            lock(&fshared).events.push(SessionEvent::Lock);
                            follower.changed();
                        }
                        (IFACE_SESSION, "Unlock") if path == state.session_path => {
                            lock(&fshared).events.push(SessionEvent::Unlock);
                            follower.changed();
                        }
                        (IFACE_MANAGER, "PrepareForSleep") => {
                            let start = msg.body().deserialize::<(bool,)>().map(|(b,)| b).unwrap_or(false);
                            manager.insert("PreparingForSleep".into(), zbus::zvariant::OwnedValue::from(start));
                            let mut shared = lock(&fshared);
                            if start {
                                shared.events.push(SessionEvent::PrepareForSleep);
                                // Nobody will release it: don't hold up sleep.
                                if !shared.want_delay {
                                    shared.delay = None;
                                }
                            } else {
                                shared.events.push(SessionEvent::Resumed);
                                if shared.want_delay && shared.delay.is_none() {
                                    shared.delay = take_delay(bus).ok();
                                }
                            }
                            dirty = true;
                        }
                        (IFACE_MANAGER, "PrepareForShutdown") => {
                            let start = msg.body().deserialize::<(bool,)>().map(|(b,)| b).unwrap_or(false);
                            manager.insert("PreparingForShutdown".into(), zbus::zvariant::OwnedValue::from(start));
                            if start {
                                lock(&fshared).events.push(SessionEvent::PrepareForShutdown);
                            }
                            dirty = true;
                        }
                        _ => {}
                    }
                }
                Event::Idle => {
                    if present && dirty {
                        let held = lock(&fshared).delay.is_some();
                        follower.set_if_changed(read_state(&state, &manager, &session_props, held));
                        dirty = false;
                    }
                }
            }
        });
        Logind { bus: Some(bus), model, shared, stop }
    }

    pub fn available(&self) -> bool {
        self.model.get().available
    }

    /// Events since the last call, oldest first.
    pub fn take_events(&self) -> Vec<SessionEvent> {
        std::mem::take(&mut lock(&self.shared).events)
    }

    fn manager(&self, method: &'static str) -> Task<()> {
        bus::spawn(&self.bus, "logind", self.model.wake(), move |bus| {
            bus.call_with(Opts::ACTION, LOGIN1, MANAGER_PATH, IFACE_MANAGER, method, &(true,))
        })
    }

    pub fn power_off(&self) -> Task<()> {
        self.manager("PowerOff")
    }

    pub fn reboot(&self) -> Task<()> {
        self.manager("Reboot")
    }

    pub fn suspend(&self) -> Task<()> {
        self.manager("Suspend")
    }

    pub fn hibernate(&self) -> Task<()> {
        self.manager("Hibernate")
    }

    pub fn hybrid_sleep(&self) -> Task<()> {
        self.manager("HybridSleep")
    }

    pub fn suspend_then_hibernate(&self) -> Task<()> {
        self.manager("SuspendThenHibernate")
    }

    fn session(&self, method: &'static str, arg: Option<bool>) -> Task<()> {
        let path = self.model.get().session_path.clone();
        bus::spawn(&self.bus, "logind", self.model.wake(), move |bus| {
            if path.is_empty() {
                return Err(Error::NotFound("no login session".into()));
            }
            match arg {
                Some(arg) => bus.call_with(Opts::ACTION, LOGIN1, &path, IFACE_SESSION, method, &(arg,)),
                None => bus.call_with(Opts::ACTION, LOGIN1, &path, IFACE_SESSION, method, &()),
            }
        })
    }

    /// Ask logind to lock this session (its `Lock` signal then comes back
    /// as [`SessionEvent::Lock`]).
    pub fn lock_session(&self) -> Task<()> {
        self.session("Lock", None)
    }

    pub fn unlock_session(&self) -> Task<()> {
        self.session("Unlock", None)
    }

    /// Tell logind whether this session is idle (the shell's idle timer).
    pub fn set_idle_hint(&self, idle: bool) -> Task<()> {
        self.session("SetIdleHint", Some(idle))
    }

    /// Tell logind whether the screen is locked.
    pub fn set_locked_hint(&self, locked: bool) -> Task<()> {
        self.session("SetLockedHint", Some(locked))
    }

    /// Hold a sleep delay inhibitor from now on, so the shell can lock the
    /// screen when [`SessionEvent::PrepareForSleep`] arrives; release it
    /// each time with [`sleep_ready`](Logind::sleep_ready). It is taken
    /// again after each resume.
    pub fn take_sleep_delay(&self) -> Result<()> {
        let bus = self.bus.as_ref().ok_or_else(|| Error::Unavailable("logind".into()))?;
        let fd = take_delay(bus)?;
        let mut shared = lock(&self.shared);
        shared.delay = Some(fd);
        shared.want_delay = true;
        drop(shared);
        self.model.update(|s| s.sleep_delay_held = true);
        Ok(())
    }

    /// The screen is locked: let the machine sleep.
    pub fn sleep_ready(&self) {
        lock(&self.shared).delay = None;
        self.model.update(|s| s.sleep_delay_held = false);
    }

    /// Stop delaying sleep altogether.
    pub fn release_sleep_delay(&self) {
        let mut shared = lock(&self.shared);
        shared.want_delay = false;
        shared.delay = None;
        drop(shared);
        self.model.update(|s| s.sleep_delay_held = false);
    }

    /// Who holds inhibitors now.
    pub fn inhibitors(&self) -> Task<Vec<Inhibitor>> {
        bus::spawn(&self.bus, "logind", self.model.wake(), |bus| {
            let list: Vec<(String, String, String, String, u32, u32)> =
                bus.call(LOGIN1, MANAGER_PATH, IFACE_MANAGER, "ListInhibitors", &())?;
            Ok(list
                .into_iter()
                .map(|(what, who, why, mode, uid, pid)| Inhibitor { what, who, why, mode, uid, pid })
                .collect())
        })
    }

    /// Inhibit `what` (`sleep`, `idle`, `shutdown`, `handle-lid-switch`…,
    /// colon-separated) with `mode` `block` or `delay`, until the returned
    /// [`Inhibition`] is dropped. The shell's "keep awake" uses `idle:sleep`.
    pub fn inhibit(&self, what: &str, why: &str, mode: &str) -> Task<Inhibition> {
        let args = (what.to_owned(), WHO.to_owned(), why.to_owned(), mode.to_owned());
        bus::spawn(&self.bus, "logind", self.model.wake(), move |bus| {
            let fd: OwnedFd = bus.call_with(Opts::ACTION, LOGIN1, MANAGER_PATH, IFACE_MANAGER, "Inhibit", &args)?;
            Ok(Inhibition { _fd: fd })
        })
    }
}

impl Drop for Logind {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

fn take_delay(bus: &Bus) -> Result<OwnedFd> {
    bus.call(
        LOGIN1,
        MANAGER_PATH,
        IFACE_MANAGER,
        "Inhibit",
        &("sleep", WHO, "Locking the screen before sleep", "delay"),
    )
}

/// The session to follow: its id and object path.
fn find_session(bus: &Bus, explicit: Option<&str>) -> Option<(String, String)> {
    let by_id = |id: &str| -> Option<(String, String)> {
        let path: OwnedObjectPath = bus.call(LOGIN1, MANAGER_PATH, IFACE_MANAGER, "GetSession", &(id,)).ok()?;
        Some((id.to_owned(), path.to_string()))
    };
    if let Some(id) = explicit {
        return by_id(id);
    }
    if let Ok(id) = std::env::var("XDG_SESSION_ID")
        && !id.is_empty()
        && let Some(found) = by_id(&id)
    {
        return Some(found);
    }
    if let Ok(path) =
        bus.call::<_, OwnedObjectPath>(LOGIN1, MANAGER_PATH, IFACE_MANAGER, "GetSessionByPID", &(std::process::id(),))
    {
        let id: String = bus.get(LOGIN1, path.as_str(), IFACE_SESSION, "Id").unwrap_or_default();
        return Some((id, path.to_string()));
    }
    // Not in a session (started by the user's service manager): logind's
    // "auto" is then the user's graphical session.
    let id: String = bus.get(LOGIN1, "/org/freedesktop/login1/session/auto", IFACE_SESSION, "Id").ok()?;
    by_id(&id)
}

fn since_epoch(micros: u64) -> Option<SystemTime> {
    (micros > 0).then(|| UNIX_EPOCH + Duration::from_micros(micros))
}

fn read_state(base: &LogindState, manager: &Props, session: &Props, held: bool) -> LogindState {
    LogindState {
        lid_closed: manager.bool("LidClosed"),
        docked: manager.bool("Docked"),
        on_external_power: manager.bool("OnExternalPower"),
        idle_hint: manager.bool("IdleHint"),
        idle_since: if manager.bool("IdleHint") { since_epoch(manager.u64("IdleSinceHint")) } else { None },
        preparing_for_sleep: manager.bool("PreparingForSleep"),
        preparing_for_shutdown: manager.bool("PreparingForShutdown"),
        session_active: session.bool("Active"),
        session_locked_hint: session.bool("LockedHint"),
        session_idle_hint: session.bool("IdleHint"),
        block_inhibited: manager.str("BlockInhibited"),
        delay_inhibited: manager.str("DelayInhibited"),
        sleep_delay_held: held,
        ..base.clone()
    }
}

/// This process's login session path, for services that act on it
/// (brightness).
pub(crate) fn session_path(bus: &Bus) -> Option<String> {
    find_session(bus, None).map(|(_, path)| path).filter(|p| !p.is_empty())
}

/// This process's login session id, for registering per-session agents.
pub(crate) fn session_id(bus: &Bus) -> Option<String> {
    find_session(bus, None).map(|(id, _)| id).filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_answers_parse() {
        assert_eq!(Can::parse("yes"), Can::Yes);
        assert_eq!(Can::parse("challenge"), Can::Challenge);
        assert_eq!(Can::parse("no"), Can::No);
        assert_eq!(Can::parse("na"), Can::NotApplicable);
        // Newer logind answers "inhibited" while a block inhibitor holds
        // the action (here, an app keeping the machine awake): the machine
        // can still sleep, so it mustn't read as unsupported.
        assert_eq!(Can::parse("inhibited"), Can::Inhibited);
        assert!(Can::Inhibited.offered());
        assert!(!Can::NotApplicable.offered() && !Can::No.offered());
    }
}
