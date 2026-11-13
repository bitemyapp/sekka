//! `lock` with fsmentry 0.4.0. The two orthogonal regions are two graphs
//! side by side (`Screen` and `Delay`). The lock region's hierarchy is nested
//! three deep, the README's way: `Screen::Lockscreen` carries the field and
//! the `Phase` graph (`Locking`, `Locked`); `Phase::Locked` carries the shake
//! and the `Check` graph (`Idle`, `Checking`). Entry and exit actions are
//! hand-written functions around the transitions; leaving the lock screen
//! hands back its data, from which the exit action reads the shake.

use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

pub const IMPLEMENTED: bool = true;

/// `Lockscreen`'s data (the superstate of `Locking` and `Locked`).
#[derive(Debug, PartialEq, Eq)]
pub struct LockscreenData {
    /// The field isn't empty.
    has_text: bool,
    phase: Phase,
}

/// `Locked`'s data (the superstate of `Idle` and `Checking`).
#[derive(Debug, PartialEq, Eq)]
pub struct LockedData {
    /// 0: still; the shake timer runs while > 0.
    shake: u8,
    check: Check,
}

pub mod check {
    fsmentry::fsmentry! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Check {
            Idle -submit-> Checking(u32) -refused-> Idle,
        }
    }
}

pub mod phase {
    use super::*;
    fsmentry::fsmentry! {
        #[derive(Debug, PartialEq, Eq)]
        pub enum Phase {
            Locking -confirm-> Locked(LockedData),
        }
    }
}

pub mod screen {
    use super::*;
    fsmentry::fsmentry! {
        #[derive(Debug, PartialEq, Eq)]
        pub enum Screen {
            Unlocked -lock-> Lockscreen(LockscreenData) -unlock-> Unlocked,
        }
    }
}

pub mod delay {
    fsmentry::fsmentry! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Delay {
            // `release` is also LogindVanished's (one edge per pair of states).
            NotHeld -take_delay-> Held -sleep-> SleepPending -release-> NotHeld,
            Held -release-> NotHeld,
            // Resumed: that sleep is over; the fd is still valid.
            SleepPending -resume-> Held,
        }
    }
}

pub use check::{Check, CheckEntry};
pub use delay::{Delay, DelayEntry};
pub use phase::{Phase, PhaseEntry};
pub use screen::{Screen, ScreenEntry};

impl Delay {
    /// The pending sleep may go ahead: the lock is confirmed, or the lock screen left.
    fn release_pending(&mut self, out: &mut impl Sink<Effect>) {
        if let DelayEntry::SleepPending(d) = self.entry() {
            out.push(Effect::ReleaseDelay);
            d.release();
        }
    }

    fn take(&mut self, config: Config, out: &mut impl Sink<Effect>) {
        if let DelayEntry::NotHeld(d) = self.entry()
            && config.lock_before_sleep
        {
            out.push(Effect::TakeDelay);
            d.take_delay();
        }
    }
}

impl Screen {
    /// `Locked`'s data, and whether the field has text, while `Locked`.
    fn locked(&mut self) -> Option<(&mut bool, &mut LockedData)> {
        match self {
            Screen::Lockscreen(LockscreenData { has_text, phase: Phase::Locked(l) }) => Some((has_text, l)),
            _ => None,
        }
    }
}

impl LockscreenData {
    /// The `Phase` graph's transition API (legal edges only, no effects).
    pub fn phase_entry(&mut self) -> PhaseEntry<'_> {
        self.phase.entry()
    }
}

impl LockedData {
    /// The `Check` graph's transition API (legal edges only, no effects).
    pub fn check_entry(&mut self) -> CheckEntry<'_> {
        self.check.entry()
    }
}

pub struct Lock {
    config: Config,
    outputs: u32,
    last_check: u32,
    screen: Screen,
    delay: Delay,
}

impl Lock {
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn delay(&self) -> &Delay {
        &self.delay
    }

    /// fsmentry's transition API on the live lock region: only legal edges
    /// (and none of `handle`'s effects).
    pub fn screen_entry(&mut self) -> ScreenEntry<'_> {
        self.screen.entry()
    }

    /// The same for the delay region.
    pub fn delay_entry(&mut self) -> DelayEntry<'_> {
        self.delay.entry()
    }

    /// Enter `Lockscreen` (at `Locking`).
    fn lock(&mut self, out: &mut impl Sink<Effect>) {
        if let ScreenEntry::Unlocked(s) = self.screen.entry() {
            out.push(Effect::RequestLock);
            let mut m = self.outputs;
            while m != 0 {
                out.push(Effect::OpenSurface(m.trailing_zeros() as u8));
                m &= m - 1;
            }
            s.lock(LockscreenData { has_text: false, phase: Phase::Locking });
        }
    }

    /// Leave `Lockscreen` (`release`: this transition ends the session lock).
    fn unlock(&mut self, release: bool, out: &mut impl Sink<Effect>) {
        if let ScreenEntry::Lockscreen(s) = self.screen.entry() {
            if let LockscreenData { phase: Phase::Locked(LockedData { shake: 1.., .. }), .. } = s.unlock() {
                out.push(Effect::CancelShake);
            }
            if release {
                out.push(Effect::ReleaseLock);
            }
            out.push(Effect::LockedHint(false));
            self.delay.release_pending(out);
        }
    }

    /// A refused password: back to `Locked.Idle`, shaking.
    fn refused(&mut self, note: LockNote, out: &mut impl Sink<Effect>) {
        let motion = self.config.motion;
        let Some((_, l)) = self.screen.locked() else { return };
        if let CheckEntry::Checking(c) = l.check.entry() {
            out.push(Effect::ShowNote(note));
            c.refused();
            if motion {
                l.shake = 1;
                out.push(Effect::ShakeStep(1));
                out.push(Effect::ArmShake);
            }
        }
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        Self { config, outputs: config.outputs, last_check: 0, screen: Screen::Unlocked, delay: Delay::NotHeld }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let shown = matches!(self.screen, Screen::Lockscreen(_));
        match event {
            Event::Lock => self.lock(out),
            Event::LockConfirmed => {
                if let ScreenEntry::Lockscreen(mut s) = self.screen.entry()
                    && let PhaseEntry::Locking(p) = s.as_mut().phase.entry()
                {
                    out.push(Effect::LockedHint(true));
                    self.delay.release_pending(out);
                    p.confirm(LockedData { shake: 0, check: Check::Idle });
                }
            }
            Event::LockEnded => self.unlock(false, out),
            Event::UnlockRequested => self.unlock(true, out),
            Event::OutputAdded(o) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit == 0 {
                    self.outputs |= bit;
                    if shown {
                        out.push(Effect::OpenSurface(o));
                    }
                }
            }
            Event::OutputRemoved(o) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit != 0 {
                    self.outputs &= !bit;
                    if shown {
                        out.push(Effect::CloseSurface(o));
                    }
                }
            }
            Event::Typed { empty } => {
                if let Some((has_text, LockedData { check: Check::Idle, .. })) = self.screen.locked() {
                    *has_text = !empty;
                }
            }
            Event::Escape => {
                if let Some((has_text, LockedData { check: Check::Idle, .. })) = self.screen.locked() {
                    *has_text = false;
                }
            }
            Event::Submit => {
                if let Some((has_text @ true, l)) = self.screen.locked()
                    && let CheckEntry::Idle(c) = l.check.entry()
                {
                    self.last_check += 1;
                    out.push(Effect::StartCheck { check: self.last_check });
                    *has_text = false;
                    c.submit(self.last_check);
                }
            }
            Event::PamDone { check, outcome } => {
                let current = matches!(self.screen.locked(), Some((_, LockedData { check: Check::Checking(c), .. })) if *c == check);
                if current {
                    match outcome {
                        PamOutcome::Accepted => self.unlock(true, out),
                        PamOutcome::WrongPassword => self.refused(LockNote::WrongPassword, out),
                        PamOutcome::Refused(r) => self.refused(LockNote::Refused(r), out),
                    }
                }
            }
            Event::ShakeTick => {
                if let Some((_, LockedData { shake: shake @ 1.., .. })) = self.screen.locked() {
                    *shake += 1;
                    if *shake >= 9 {
                        *shake = 0;
                        out.push(Effect::ShakeStep(0));
                    } else {
                        out.push(Effect::ShakeStep(*shake));
                        out.push(Effect::ArmShake);
                    }
                }
            }
            Event::SessionStarted | Event::LogindAppeared => self.delay.take(self.config, out),
            Event::PrepareForSleep => {
                if !self.config.lock_before_sleep {
                    return;
                }
                self.lock(out);
                let locking = matches!(self.screen, Screen::Lockscreen(LockscreenData { phase: Phase::Locking, .. }));
                if let DelayEntry::Held(d) = self.delay.entry() {
                    if locking {
                        d.sleep();
                    } else {
                        out.push(Effect::ReleaseDelay);
                        d.release();
                    }
                }
            }
            Event::Resumed => match self.delay.entry() {
                DelayEntry::SleepPending(d) => d.resume(),
                DelayEntry::NotHeld(_) => self.delay.take(self.config, out),
                DelayEntry::Held(_) => {}
            },
            Event::LogindVanished => match self.delay.entry() {
                DelayEntry::Held(d) => d.release(),
                DelayEntry::SleepPending(d) => d.release(),
                DelayEntry::NotHeld(_) => {}
            },
        }
    }

    fn state(&self) -> StateName {
        StateName {
            lock: match &self.screen {
                Screen::Unlocked => LockPhase::Unlocked,
                Screen::Lockscreen(LockscreenData { phase: Phase::Locking, .. }) => LockPhase::Locking,
                Screen::Lockscreen(LockscreenData { phase: Phase::Locked(LockedData { check, .. }), .. }) => {
                    match check {
                        Check::Idle => LockPhase::Locked,
                        Check::Checking(_) => LockPhase::Checking,
                    }
                }
            },
            delay: match self.delay {
                Delay::NotHeld => DelayPhase::NotHeld,
                Delay::Held => DelayPhase::Held,
                Delay::SleepPending => DelayPhase::SleepPending,
            },
        }
    }
}
