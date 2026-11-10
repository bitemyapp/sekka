//! Idleness: after the configured minutes without input, subscribers hear
//! `idle` `lock` (the shell locks) and then `idle` `monitors-off` (the
//! displays power off); the next input sends `resumed`. Clients get
//! ext-idle-notify from smithay alongside.
//!
//! Two kinds of hold keep idleness off ([`Holding`]). An app's is a
//! Wayland idle inhibitor on one of its surfaces (a video player's): it
//! counts until the session is locked, so a locked screen goes dark on its
//! timer whatever plays behind it. Keep Awake's is the user's own switch,
//! asked for over IPC (`keep-awake`) on a connection it keeps open: it
//! counts locked or not. IPC says who holds (`idle-inhibitors`, and
//! `idle-inhibitors-changed` when that changes).

use std::time::{Duration, Instant};

use aizome_config::Settings;
use aizome_ipc::{Event, IdleHolder, IdleInhibitorInfo};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::get_parent;

use crate::state::{LockState, State};

#[derive(Debug)]
pub struct IdleState {
    pub last_activity: Instant,
    lock_after: Option<Duration>,
    off_after: Option<Duration>,
    lock_sent: bool,
    off_sent: bool,
    /// Who held idleness off when IPC subscribers were last told.
    reported: Vec<IdleInhibitorInfo>,
}

impl Default for IdleState {
    fn default() -> Self {
        IdleState {
            last_activity: Instant::now(),
            lock_after: None,
            off_after: None,
            lock_sent: false,
            off_sent: false,
            reported: Vec::new(),
        }
    }
}

impl IdleState {
    pub fn reset_timers(&mut self, s: &Settings) {
        let m = |n: u32| (n > 0).then(|| Duration::from_secs(n as u64 * 60));
        self.lock_after = m(s.idle.lock_after_minutes);
        self.off_after = m(s.idle.monitors_off_after_minutes);
    }
}

/// The surfaces with an idle inhibitor (apps' holds), in the order each
/// took its first.
#[derive(Debug, Default)]
pub struct IdleInhibitors(Vec<WlSurface>);

impl IdleInhibitors {
    /// `surface` holds from now on; whether it's news.
    fn insert(&mut self, surface: WlSurface) -> bool {
        let new = !self.0.contains(&surface);
        if new {
            self.0.push(surface);
        }
        new
    }

    /// `surface` holds no longer; whether it did.
    fn remove(&mut self, surface: &WlSurface) -> bool {
        let before = self.0.len();
        self.0.retain(|s| s != surface);
        self.0.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = &WlSurface> {
        self.0.iter()
    }
}

/// Whose holds on idleness are in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holding {
    /// The user's own Keep Awake.
    pub keep_awake: bool,
    /// At least one app's idle inhibitor.
    pub apps: bool,
}

impl Holding {
    /// Whether anything holds at all: what ext-idle-notify's clients are
    /// told, locked or not.
    pub fn any(self) -> bool {
        self.keep_awake || self.apps
    }

    /// Whether the idle lock and the displays' turning off are held off.
    /// Keep Awake holds them off always. An app holds them off for the
    /// person at the screen: once the session is locked nobody is watching
    /// what it plays, and the displays turn off on their timer.
    pub fn holds_off_idleness(self, lock: &LockState) -> bool {
        self.keep_awake || (self.apps && !lock.is_locked())
    }
}

/// Checks idleness every few seconds.
pub fn start(state: &mut State) {
    let s = state.az.settings.clone();
    state.az.idle.reset_timers(&s);
    let _ = state.az.lh.insert_source(Timer::from_duration(Duration::from_secs(5)), |_, _, state: &mut State| {
        state.check_idle();
        TimeoutAction::ToDuration(Duration::from_secs(5))
    });
}

impl State {
    /// Input happened.
    pub fn note_activity(&mut self) {
        let seat = self.az.seat.clone();
        self.az.idle_notifier.notify_activity(&seat);
        let idle = &mut self.az.idle;
        idle.last_activity = Instant::now();
        if idle.lock_sent || idle.off_sent {
            let was_off = idle.off_sent;
            idle.lock_sent = false;
            idle.off_sent = false;
            self.az.ipc.emit(&Event::Resumed);
            if was_off {
                self.set_monitors_power(true);
            }
        }
    }

    /// `surface` holds idleness off: its client made an idle inhibitor on
    /// it.
    pub fn add_idle_inhibitor(&mut self, surface: WlSurface) {
        if self.az.idle_inhibitors.insert(surface) {
            self.idle_holds_changed();
        }
    }

    /// `surface` no longer holds idleness off: its inhibitor was destroyed,
    /// or the surface itself was. smithay forgets an inhibitor only when
    /// its client destroys it, so one whose client died (a crashed video
    /// player) goes with its surface.
    pub fn drop_idle_inhibitor(&mut self, surface: &WlSurface) {
        if self.az.idle_inhibitors.remove(surface) {
            self.idle_holds_changed();
        }
    }

    /// Whose holds are in force now.
    pub fn holding(&self) -> Holding {
        Holding { keep_awake: self.az.ipc.keeps_awake(), apps: !self.az.idle_inhibitors.is_empty() }
    }

    /// Who holds idleness off: apps in the order their holds began, then
    /// Keep Awake's connections. An app's surface is named by the window
    /// it belongs to, when it's a window's; a bare one (the shell's, for
    /// apps that asked the session bus) only by its client's process.
    pub fn idle_inhibitors(&self) -> Vec<IdleInhibitorInfo> {
        let apps = self.az.idle_inhibitors.iter().map(|surface| {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            let window = self.az.window_for_surface(&root);
            let ids = window.and_then(|id| self.az.windows.get(&id)).map(|m| crate::shell::toplevel_ids(m.toplevel()));
            let (app_id, title) = ids.unzip();
            let pid = self
                .az
                .dh
                .get_client(surface.id())
                .ok()
                .and_then(|client| client.get_credentials(&self.az.dh).ok())
                .map(|credentials| credentials.pid);
            let said = |text: Option<String>| text.filter(|t| !t.is_empty());
            IdleInhibitorInfo { kind: IdleHolder::App, window, app_id: said(app_id), title: said(title), pid }
        });
        let keep_awake = self.az.ipc.keep_awake_pids().map(|pid| IdleInhibitorInfo {
            kind: IdleHolder::KeepAwake,
            pid,
            ..Default::default()
        });
        apps.chain(keep_awake).collect()
    }

    /// A hold began or ended (or may have): ext-idle-notify's clients hear
    /// whether any is left, and IPC subscribers who holds now.
    pub fn idle_holds_changed(&mut self) {
        let any = self.holding().any();
        self.az.idle_notifier.set_is_inhibited(any);
        self.report_idle_inhibitors();
    }

    /// Tells IPC subscribers who holds idleness off, when that's another
    /// answer than they last had: a hold began or ended, or a holder's
    /// window opened or was renamed since.
    fn report_idle_inhibitors(&mut self) {
        let now = self.idle_inhibitors();
        if now != self.az.idle.reported {
            self.az.idle.reported = now.clone();
            self.az.ipc.emit(&Event::IdleInhibitorsChanged { inhibitors: now });
        }
    }

    /// As if nobody had touched anything for `idle_for`: the clock is put
    /// back and read at once (IPC's `synthesize`, for tests and scripts).
    pub fn synthesize_idle(&mut self, idle_for: Duration) {
        let now = Instant::now();
        self.az.idle.last_activity = now.checked_sub(idle_for).unwrap_or(now);
        self.check_idle();
    }

    fn check_idle(&mut self) {
        if self.az.ipc.take_keep_awake_changed() {
            self.idle_holds_changed();
        } else {
            // A holder's window may have opened, or its title changed.
            self.report_idle_inhibitors();
        }
        if self.holding().holds_off_idleness(&self.az.lock) {
            self.az.idle.last_activity = Instant::now();
            return;
        }
        let idle_for = self.az.idle.last_activity.elapsed();
        if let Some(d) = self.az.idle.lock_after
            && idle_for >= d
            && !self.az.idle.lock_sent
        {
            self.az.idle.lock_sent = true;
            self.az.ipc.emit(&Event::Idle { kind: "lock".into() });
        }
        if let Some(d) = self.az.idle.off_after
            && idle_for >= d
            && !self.az.idle.off_sent
        {
            self.az.idle.off_sent = true;
            self.az.ipc.emit(&Event::Idle { kind: "monitors-off".into() });
            self.set_monitors_power(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apps_hold_counts_until_the_session_is_locked_and_keep_awakes_always() {
        let app = Holding { keep_awake: false, apps: true };
        let keep_awake = Holding { keep_awake: true, apps: false };
        let both = Holding { keep_awake: true, apps: true };
        let nothing = Holding { keep_awake: false, apps: false };
        // At the screen, either holds the lock and the displays off.
        for holding in [app, keep_awake, both] {
            assert!(holding.holds_off_idleness(&LockState::Unlocked), "{holding:?}");
        }
        assert!(!nothing.holds_off_idleness(&LockState::Unlocked));
        // Locked, only the user's own switch does.
        assert!(!app.holds_off_idleness(&LockState::Locked), "an app kept a locked screen lit");
        assert!(keep_awake.holds_off_idleness(&LockState::Locked));
        assert!(both.holds_off_idleness(&LockState::Locked));
        assert!(!nothing.holds_off_idleness(&LockState::Locked));
        // Idle daemons are told of any hold, locked or not.
        assert!(app.any() && keep_awake.any() && both.any() && !nothing.any());
    }
}
