//! Idleness: after the configured minutes without input, subscribers hear
//! `idle` `lock` (the shell locks) and then `idle` `monitors-off` (the
//! displays power off); the next input sends `resumed`. Idle-inhibit
//! surfaces (video players) hold it off. Clients get ext-idle-notify from
//! smithay alongside.

use std::time::{Duration, Instant};

use aizome_config::Settings;
use aizome_ipc::Event;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

use crate::state::State;

#[derive(Debug)]
pub struct IdleState {
    pub last_activity: Instant,
    lock_after: Option<Duration>,
    off_after: Option<Duration>,
    lock_sent: bool,
    off_sent: bool,
}

impl Default for IdleState {
    fn default() -> Self {
        IdleState {
            last_activity: Instant::now(),
            lock_after: None,
            off_after: None,
            lock_sent: false,
            off_sent: false,
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
                self.backend.set_power(&mut self.az, true);
            }
        }
    }

    /// `surface` no longer holds idleness off: its inhibitor was destroyed,
    /// or the surface itself was. smithay forgets an inhibitor only when
    /// its client destroys it, so one whose client died (Keep Awake's
    /// holder killed, a crashed video player) goes with its surface.
    pub fn drop_idle_inhibitor(&mut self, surface: &WlSurface) {
        if self.az.idle_inhibitors.remove(surface) {
            let any = !self.az.idle_inhibitors.is_empty();
            self.az.idle_notifier.set_is_inhibited(any);
        }
    }

    fn check_idle(&mut self) {
        if !self.az.idle_inhibitors.is_empty() {
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
            self.backend.set_power(&mut self.az, false);
        }
    }
}
