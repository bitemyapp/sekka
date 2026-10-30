//! Commits that wait for the display: wp_fifo_v1 barriers and
//! wp_commit_timing_v1 timestamps. smithay holds such a commit back with a
//! blocker; letting it through is the compositor's part, and a commit
//! nothing lets through stays back for good, with every commit after it.
//! A Vulkan swapchain presenting with vsync (FIFO) waits on a barrier with
//! every frame, so without this it froze after its first.
//!
//! - A barrier clears after a frame on the surface's output, as its frame
//!   callbacks go ([`frame_drawn`]): a swapchain with vsync gets a frame a
//!   refresh. Where no frame comes (a surface no output shows: minimised,
//!   on another Desk, covered; or a display that isn't drawing) it clears
//!   within about a second instead ([`BACKSTOP`]), as such a surface's
//!   frame callbacks come, so the client still makes progress.
//! - A timestamped commit joins the first frame that reaches the screen at
//!   or after its time ([`State::release_timed`], just before a frame is
//!   drawn), and a timer wakes for the vblank before that time. A surface
//!   no output shows gets its commit at the time itself.

use std::time::Duration;

use smithay::desktop::layer_map_for_output;
use smithay::desktop::utils::{surface_primary_scanout_output, with_surfaces_surface_tree};
use smithay::input::pointer::CursorImageStatus;
use smithay::output::Output;
use smithay::reexports::calloop::RegistrationToken;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Resource, Weak};
use smithay::utils::{Monotonic, Time};
use smithay::wayland::commit_timing::{CommitTimerBarrierStateUserData, CommitTimerStateUserData, Timestamp};
use smithay::wayland::compositor::{Barrier, CompositorHandler, SurfaceData, is_sync_subsurface, with_states};
use smithay::wayland::fifo::{FifoBarrierCachedState, FifoCachedState};

use crate::state::{Aizome, State};

/// The longest a barrier waits where no frame clears it, and how long an
/// output that drew counts as drawing.
const BACKSTOP: Duration = Duration::from_secs(1);

/// How long after a vblank the next ones are counted from it.
const PREDICTED: Duration = Duration::from_secs(1);

/// The timers, and the clients a frame let through.
#[derive(Default)]
pub struct Pacing {
    /// Clears the barriers no frame does; set while a commit waits on one.
    backstop: Option<RegistrationToken>,
    /// Actual FIFO waits, including barriers in commits not applied yet.
    fifo_waiters: Vec<(Weak<WlSurface>, Barrier)>,
    /// Wakes for the earliest timestamped commit, and when (monotonic).
    timed: Option<(RegistrationToken, Duration)>,
    /// Clients with barriers the last frame cleared.
    released: Vec<Client>,
}

/// Whose frames a surface goes with.
enum Shown {
    /// An output's: its primary output (windows and layer surfaces, with
    /// their popups and subsurfaces), or a lock surface's output.
    On(Output),
    /// Every output's (the pointer, a drag icon, windows not mapped yet),
    /// as their frame callbacks go.
    Anywhere,
    /// None: it doesn't show.
    Nowhere,
}

impl Shown {
    /// Whether a frame on `output` shows it.
    fn on(&self, output: &Output) -> bool {
        match self {
            Shown::On(o) => o == output,
            Shown::Anywhere => true,
            Shown::Nowhere => false,
        }
    }
}

/// A commit is coming in. smithay's fifo and commit-timing hooks run after
/// this one (a surface gets them with its fifo or timer object, after it
/// was made and got this), so what the commit asks is still pending: if it
/// will wait, makes sure something lets it through.
pub fn commit_requested(state: &mut State, surface: &WlSurface) {
    let sync_subsurface = is_sync_subsurface(surface);
    let (barrier, at, output) = with_states(surface, |states| {
        let cache = &states.cached_state;
        let barrier = if !sync_subsurface
            && cache.has::<FifoCachedState>()
            && cache.get::<FifoCachedState>().pending().wait_barrier
        {
            cache.get::<FifoBarrierCachedState>().pending().barrier.as_ref().filter(|b| !b.is_signaled()).cloned()
        } else {
            None
        };
        let at = states.data_map.get::<CommitTimerStateUserData>().and_then(|t| t.borrow().timestamp);
        (barrier, at, surface_primary_scanout_output(surface, states))
    });
    if let Some(barrier) = barrier {
        state.az.pacing.fifo_waiters.push((surface.downgrade(), barrier));
        // Its output's next frame clears the barrier; the backstop clears
        // it if that frame doesn't come.
        if let Some(o) = &output {
            state.az.queue_redraw(o);
        }
        arm_backstop(state);
    }
    if let Some(at) = at {
        let at = monotonic(at);
        let wake = match &output {
            Some(o) => release_time(&state.az, o, at),
            None => at,
        };
        arm_timed(state, wake);
    }
}

/// After a frame on `output`: clears the barriers of what it showed, with
/// their frame callbacks, for [`State::release_cleared`] to let the
/// commits waiting on them through.
pub fn frame_drawn(az: &mut Aizome, output: &Output) {
    let mut released = std::mem::take(&mut az.pacing.released);
    each_surface(az, &mut |surface, states, shown| {
        if shown.on(output) && clear_barrier(states) {
            add(&mut released, surface);
        }
    });
    az.pacing.released = released;
}

impl State {
    /// Lets through the commits waiting on barriers frames cleared.
    /// Whether there were any.
    pub fn release_cleared(&mut self) -> bool {
        let clients = std::mem::take(&mut self.az.pacing.released);
        unblock(self, clients)
    }

    /// Before a frame on `output`: lets through the timestamped commits of
    /// what it shows that are due when the frame reaches the screen, so
    /// they're in it. Whether there were any.
    pub fn release_timed(&mut self, output: &Output) -> bool {
        let target = next_vblank(&self.az, output, self.az.now());
        let mut clients = Vec::new();
        let mut later: Option<Duration> = None;
        each_surface(&self.az, &mut |surface, states, shown| {
            if !shown.on(output) {
                return;
            }
            let Some(timers) = states.data_map.get::<CommitTimerBarrierStateUserData>() else { return };
            let mut timers = timers.lock().unwrap();
            if timers.signal_until(Time::<Monotonic>::from(target)) {
                add(&mut clients, surface);
            }
            if let Some(at) = timers.next_deadline() {
                later = Some(later.map_or(monotonic(at), |l| l.min(monotonic(at))));
            }
        });
        if let Some(at) = later {
            let wake = release_time(&self.az, output, at);
            arm_timed(self, wake);
        }
        unblock(self, clients)
    }

    /// The backstop: clears the barriers of surfaces no output is drawing.
    fn backstop_due(&mut self) {
        let mut clients = Vec::new();
        let az = &self.az;
        each_surface(az, &mut |surface, states, shown| {
            if drawn_by(az, &shown).is_none() && clear_barrier(states) {
                add(&mut clients, surface);
            }
        });
        unblock(self, clients);
        // A barrier can still be waiting for its commit's timestamp, or
        // its output can have stopped drawing less than BACKSTOP ago.
        // Neither clears a barrier this tick, but both need another tick.
        self.az.pacing.fifo_waiters.retain(|(surface, barrier)| surface.is_alive() && !barrier.is_signaled());
        if !self.az.pacing.fifo_waiters.is_empty() {
            arm_backstop(self);
        }
    }

    /// The timestamped commits' timer: a surface an output is drawing gets
    /// a frame, which lets its due commits through; another gets them now.
    fn timed_due(&mut self) {
        let now = self.az.now();
        let mut clients = Vec::new();
        let mut frames: Vec<Output> = Vec::new();
        let mut wake: Option<Duration> = None;
        let mut wake_at = |t: Duration| wake = Some(wake.map_or(t, |w| w.min(t)));
        let az = &self.az;
        each_surface(az, &mut |surface, states, shown| {
            let Some(timers) = states.data_map.get::<CommitTimerBarrierStateUserData>() else { return };
            let mut timers = timers.lock().unwrap();
            match drawn_by(az, &shown) {
                Some(o) => {
                    let Some(at) = timers.next_deadline() else { return };
                    let release = release_time(az, &o, monotonic(at));
                    if release <= now {
                        if !frames.contains(&o) {
                            frames.push(o);
                        }
                        // In case that frame doesn't come (the display
                        // went off): by then the output isn't drawing.
                        wake_at(now + BACKSTOP);
                    } else {
                        wake_at(release);
                    }
                }
                None => {
                    if timers.signal_until(Time::<Monotonic>::from(now)) {
                        add(&mut clients, surface);
                    }
                    if let Some(at) = timers.next_deadline() {
                        wake_at(monotonic(at));
                    }
                }
            }
        });
        for o in &frames {
            self.az.queue_redraw(o);
        }
        if let Some(t) = wake {
            arm_timed(self, t);
        }
        unblock(self, clients);
    }
}

/// Calls `f` with every client surface and whose frames it goes with.
fn each_surface(az: &Aizome, f: &mut dyn FnMut(&WlSurface, &SurfaceData, Shown)) {
    let primary = |surface: &WlSurface, states: &SurfaceData| {
        surface_primary_scanout_output(surface, states).map_or(Shown::Nowhere, Shown::On)
    };
    for m in az.windows.values() {
        m.window.with_surfaces(|s, d| f(s, d, primary(s, d)));
    }
    for output in &az.outputs {
        let map = layer_map_for_output(output);
        for layer in map.layers() {
            layer.with_surfaces(|s, d| f(s, d, primary(s, d)));
        }
        drop(map);
        if let Some(lock) = az.out.get(output).and_then(|o| o.lock_surface.as_ref()) {
            with_surfaces_surface_tree(lock.wl_surface(), |s, d| f(s, d, Shown::On(output.clone())));
        }
    }
    let mut anywhere = |surface: &WlSurface| with_surfaces_surface_tree(surface, |s, d| f(s, d, Shown::Anywhere));
    if let CursorImageStatus::Surface(s) = &az.cursor_status {
        anywhere(s);
    }
    if let Some(icon) = &az.dnd_icon {
        anywhere(icon);
    }
    for u in &az.unmapped {
        if let Some(t) = u.window.toplevel() {
            anywhere(t.wl_surface());
        }
    }
}

/// Clears a surface's barrier if one is set; whether one was.
fn clear_barrier(states: &SurfaceData) -> bool {
    if !states.cached_state.has::<FifoBarrierCachedState>() {
        return false;
    }
    let barrier = states.cached_state.get::<FifoBarrierCachedState>().current().barrier.take();
    match barrier {
        Some(b) if !b.is_signaled() => {
            b.signal();
            true
        }
        _ => false,
    }
}

fn add(clients: &mut Vec<Client>, surface: &WlSurface) {
    if let Some(client) = surface.client()
        && !clients.iter().any(|c| c.id() == client.id())
    {
        clients.push(client);
    }
}

/// Lets clients' ready commits through. Whether there were clients.
fn unblock(state: &mut State, clients: Vec<Client>) -> bool {
    let any = !clients.is_empty();
    let dh = state.az.dh.clone();
    for client in clients {
        state.client_compositor_state(&client).blocker_cleared(state, &dh);
    }
    any
}

/// Whether an output drew a frame lately (so its next frames will come).
fn drawing(az: &Aizome, output: &Output) -> bool {
    az.out.get(output).and_then(|o| o.last_frame).is_some_and(|t| t.elapsed() < BACKSTOP)
}

/// The output drawing a surface, if one is.
fn drawn_by(az: &Aizome, shown: &Shown) -> Option<Output> {
    match shown {
        Shown::On(o) => drawing(az, o).then(|| o.clone()),
        Shown::Anywhere => az.outputs.iter().find(|o| drawing(az, o)).cloned(),
        Shown::Nowhere => None,
    }
}

fn arm_backstop(state: &mut State) {
    if state.az.pacing.backstop.is_some() {
        return;
    }
    let token = state.az.lh.insert_source(Timer::from_duration(BACKSTOP), |_, _, state: &mut State| {
        state.az.pacing.backstop = None;
        state.backstop_due();
        TimeoutAction::Drop
    });
    state.az.pacing.backstop = token.ok();
}

/// Wakes the timestamped commits' timer at `at` (monotonic), unless it
/// wakes sooner already.
fn arm_timed(state: &mut State, at: Duration) {
    if state.az.pacing.timed.as_ref().is_some_and(|&(_, t)| t <= at) {
        return;
    }
    if let Some((token, _)) = state.az.pacing.timed.take() {
        state.az.lh.remove(token);
    }
    let after = at.saturating_sub(state.az.now());
    let token = state.az.lh.insert_source(Timer::from_duration(after), |_, _, state: &mut State| {
        state.az.pacing.timed = None;
        state.timed_due();
        TimeoutAction::Drop
    });
    state.az.pacing.timed = token.ok().map(|t| (t, at));
}

fn monotonic(t: Timestamp) -> Duration {
    Duration::from(Time::<Monotonic>::from(t))
}

/// The last vblank an output reported and its refresh, while they're
/// recent enough to count from.
fn vblanks(az: &Aizome, output: &Output) -> Option<(Duration, Duration)> {
    let o = az.out.get(output)?;
    if o.vrr_enabled {
        return None;
    }
    let last = o.presented?;
    (az.now().saturating_sub(last) < PREDICTED).then_some((last, o.refresh))
}

/// When a frame drawn at `now` on an output reaches the screen: its next
/// vblank, or `now` where there are no vblanks to count (so a commit comes
/// up to a refresh late rather than early).
fn next_vblank(az: &Aizome, output: &Output, now: Duration) -> Duration {
    match vblanks(az, output) {
        Some((last, refresh)) => vblank_after(last, refresh, now),
        None => now,
    }
}

/// When to let a commit for `at` through on an output: at the vblank
/// before `at`, as a frame drawn after it reaches the screen at or after
/// `at`; or at `at` where there are no vblanks to count.
fn release_time(az: &Aizome, output: &Output, at: Duration) -> Duration {
    match vblanks(az, output) {
        Some((last, refresh)) => vblank_before(last, refresh, at),
        None => at,
    }
}

/// The first vblank after `t`, with vblanks every `refresh` from `last`.
fn vblank_after(last: Duration, refresh: Duration, t: Duration) -> Duration {
    let r = refresh.as_nanos().max(1);
    let n = t.saturating_sub(last).as_nanos() / r + 1;
    last + Duration::from_nanos((n * r) as u64)
}

/// The last vblank before `t` (no earlier than `last`).
fn vblank_before(last: Duration, refresh: Duration, t: Duration) -> Duration {
    let r = refresh.as_nanos().max(1);
    let n = t.saturating_sub(last).as_nanos().div_ceil(r).saturating_sub(1);
    last + Duration::from_nanos((n * r) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Duration = Duration::from_millis(16);
    const LAST: Duration = Duration::from_secs(100);

    #[test]
    fn a_frame_drawn_between_vblanks_reaches_the_next() {
        assert_eq!(vblank_after(LAST, R, LAST), LAST + R);
        assert_eq!(vblank_after(LAST, R, LAST + R / 2), LAST + R);
        assert_eq!(vblank_after(LAST, R, LAST + R), LAST + R * 2);
        assert_eq!(vblank_after(LAST, R, LAST + R * 10 + R / 3), LAST + R * 11);
    }

    #[test]
    fn a_commit_goes_in_after_the_vblank_before_its_time() {
        // Drawn after LAST + R, a frame shows at LAST + 2R.
        assert_eq!(vblank_before(LAST, R, LAST + R * 2), LAST + R);
        assert_eq!(vblank_before(LAST, R, LAST + R + R / 2), LAST + R);
        // Due by the next vblank, or already: now.
        assert_eq!(vblank_before(LAST, R, LAST + R / 2), LAST);
        assert_eq!(vblank_before(LAST, R, LAST - R), LAST);
    }

    #[test]
    fn released_at_that_vblank_it_reaches_the_screen_at_or_after_its_time() {
        for at in [LAST + R * 3, LAST + R * 3 + Duration::from_micros(1), LAST + R * 4 - Duration::from_micros(1)] {
            let released = vblank_before(LAST, R, at);
            let shown = vblank_after(LAST, R, released);
            assert!(shown >= at && shown < at + R, "for {at:?}: released {released:?}, shown {shown:?}");
        }
    }
}
