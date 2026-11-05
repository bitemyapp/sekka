//! `lock` as hand-written typestate: the lock region's states are types,
//! and logind's delay is a token (`NoDelay`, `DelayFd`, `PendingSleep`) so
//! it can only be taken when none is held, and a pending sleep can only be
//! released by confirming the lock or leaving the lock screen.

use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

pub const IMPLEMENTED: bool = true;

/// No inhibitor held.
#[derive(Debug)]
pub struct NoDelay(());
/// logind's sleep-delay inhibitor fd.
#[derive(Debug)]
pub struct DelayFd(());
/// logind waits to sleep; the fd is held until the lock is confirmed.
#[derive(Debug)]
pub struct PendingSleep(DelayFd);

impl NoDelay {
    pub fn take(self, out: &mut impl Sink<Effect>) -> DelayFd {
        out.push(Effect::TakeDelay);
        DelayFd(())
    }
}

impl DelayFd {
    /// Let logind sleep now.
    pub fn release(self, out: &mut impl Sink<Effect>) -> NoDelay {
        out.push(Effect::ReleaseDelay);
        NoDelay(())
    }
    pub fn hold_for_sleep(self) -> PendingSleep {
        PendingSleep(self)
    }
    /// logind went away with it.
    pub fn lost(self) -> NoDelay {
        NoDelay(())
    }
}

impl PendingSleep {
    /// That sleep is over; the fd still holds for the next one.
    pub fn resumed(self) -> DelayFd {
        self.0
    }
    pub fn lost(self) -> NoDelay {
        NoDelay(())
    }
    fn release(self, out: &mut impl Sink<Effect>) -> NoDelay {
        self.0.release(out)
    }
}

/// The delay region.
#[derive(Debug)]
pub enum Delay {
    NotHeld(NoDelay),
    Held(DelayFd),
    SleepPending(PendingSleep),
}

impl Delay {
    /// Leaving `Locking` (confirmed, or the lock screen ends): a pending sleep goes ahead.
    fn settle(self, out: &mut impl Sink<Effect>) -> Delay {
        match self {
            Delay::SleepPending(p) => Delay::NotHeld(p.release(out)),
            d => d,
        }
    }
}

#[derive(Debug)]
pub struct Unlocked(());
#[derive(Debug)]
pub struct Locking(());
#[derive(Debug)]
pub struct Locked {
    has_text: bool,
    shake: u8,
}
#[derive(Debug)]
pub struct Checking {
    shake: u8,
    check: u32,
}

fn shake_tick(shake: &mut u8, out: &mut impl Sink<Effect>) {
    if *shake == 0 {
        return;
    }
    *shake += 1;
    if *shake >= 9 {
        *shake = 0;
        out.push(Effect::ShakeStep(0));
    } else {
        out.push(Effect::ShakeStep(*shake));
        out.push(Effect::ArmShake);
    }
}

fn leave(shake: u8, release: bool, delay: Delay, out: &mut impl Sink<Effect>) -> (Unlocked, Delay) {
    if shake > 0 {
        out.push(Effect::CancelShake);
    }
    if release {
        out.push(Effect::ReleaseLock);
    }
    out.push(Effect::LockedHint(false));
    (Unlocked(()), delay.settle(out))
}

impl Unlocked {
    pub fn new() -> Self {
        Unlocked(())
    }
    pub fn lock(self, outputs: u32, out: &mut impl Sink<Effect>) -> Locking {
        out.push(Effect::RequestLock);
        let mut m = outputs;
        while m != 0 {
            out.push(Effect::OpenSurface(m.trailing_zeros() as u8));
            m &= m - 1;
        }
        Locking(())
    }
}

impl Default for Unlocked {
    fn default() -> Self {
        Self::new()
    }
}

impl Locking {
    pub fn confirm(self, delay: Delay, out: &mut impl Sink<Effect>) -> (Locked, Delay) {
        out.push(Effect::LockedHint(true));
        (Locked { has_text: false, shake: 0 }, delay.settle(out))
    }
    pub fn leave(self, release: bool, delay: Delay, out: &mut impl Sink<Effect>) -> (Unlocked, Delay) {
        leave(0, release, delay, out)
    }
}

impl Locked {
    pub fn typed(&mut self, empty: bool) {
        self.has_text = !empty;
    }
    pub fn submit(self, last_check: &mut u32, out: &mut impl Sink<Effect>) -> Result<Checking, Locked> {
        if !self.has_text {
            return Err(self);
        }
        *last_check += 1;
        out.push(Effect::StartCheck { check: *last_check });
        Ok(Checking { shake: self.shake, check: *last_check })
    }
    pub fn tick(&mut self, out: &mut impl Sink<Effect>) {
        shake_tick(&mut self.shake, out);
    }
    pub fn leave(self, release: bool, delay: Delay, out: &mut impl Sink<Effect>) -> (Unlocked, Delay) {
        leave(self.shake, release, delay, out)
    }
}

impl Checking {
    pub fn check(&self) -> u32 {
        self.check
    }
    pub fn tick(&mut self, out: &mut impl Sink<Effect>) {
        shake_tick(&mut self.shake, out);
    }
    pub fn accepted(self, delay: Delay, out: &mut impl Sink<Effect>) -> (Unlocked, Delay) {
        leave(self.shake, true, delay, out)
    }
    pub fn refused(self, note: LockNote, motion: bool, out: &mut impl Sink<Effect>) -> Locked {
        out.push(Effect::ShowNote(note));
        let shake = if motion {
            out.push(Effect::ShakeStep(1));
            out.push(Effect::ArmShake);
            1
        } else {
            0
        };
        Locked { has_text: false, shake }
    }
    pub fn leave(self, release: bool, delay: Delay, out: &mut impl Sink<Effect>) -> (Unlocked, Delay) {
        leave(self.shake, release, delay, out)
    }
}

/// The lock region's runtime wrapper.
#[derive(Debug)]
pub enum Phase {
    Unlocked(Unlocked),
    Locking(Locking),
    Locked(Locked),
    Checking(Checking),
}

pub struct Lock {
    config: Config,
    outputs: u32,
    last_check: u32,
    phase: Phase,
    delay: Delay,
}

impl Lock {
    pub fn typed(&self) -> (&Phase, &Delay) {
        (&self.phase, &self.delay)
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        Self {
            config,
            outputs: config.outputs,
            last_check: 0,
            phase: Phase::Unlocked(Unlocked(())),
            delay: Delay::NotHeld(NoDelay(())),
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use Phase as P;
        let phase = std::mem::replace(&mut self.phase, P::Unlocked(Unlocked(())));
        let delay = std::mem::replace(&mut self.delay, Delay::NotHeld(NoDelay(())));
        let want = self.config.lock_before_sleep;
        let (phase, delay) = match (phase, event) {
            (P::Unlocked(s), Event::Lock) => (P::Locking(s.lock(self.outputs, out)), delay),
            (P::Locking(s), Event::LockConfirmed) => {
                let (l, d) = s.confirm(delay, out);
                (P::Locked(l), d)
            }
            (p, Event::LockEnded | Event::UnlockRequested) => {
                let release = event == Event::UnlockRequested;
                let (u, d) = match p {
                    P::Unlocked(s) => (s, delay),
                    P::Locking(s) => s.leave(release, delay, out),
                    P::Locked(s) => s.leave(release, delay, out),
                    P::Checking(s) => s.leave(release, delay, out),
                };
                (P::Unlocked(u), d)
            }
            (p, Event::OutputAdded(o)) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit == 0 {
                    self.outputs |= bit;
                    if !matches!(p, P::Unlocked(_)) {
                        out.push(Effect::OpenSurface(o));
                    }
                }
                (p, delay)
            }
            (p, Event::OutputRemoved(o)) => {
                let bit = 1u32 << (o & 31);
                if self.outputs & bit != 0 {
                    self.outputs &= !bit;
                    if !matches!(p, P::Unlocked(_)) {
                        out.push(Effect::CloseSurface(o));
                    }
                }
                (p, delay)
            }
            (P::Locked(mut s), Event::Typed { empty }) => {
                s.typed(empty);
                (P::Locked(s), delay)
            }
            (P::Locked(mut s), Event::Escape) => {
                s.typed(true);
                (P::Locked(s), delay)
            }
            (P::Locked(s), Event::Submit) => match s.submit(&mut self.last_check, out) {
                Ok(c) => (P::Checking(c), delay),
                Err(l) => (P::Locked(l), delay),
            },
            (P::Checking(s), Event::PamDone { check, outcome }) if check == s.check() => match outcome {
                PamOutcome::Accepted => {
                    let (u, d) = s.accepted(delay, out);
                    (P::Unlocked(u), d)
                }
                PamOutcome::WrongPassword => {
                    (P::Locked(s.refused(LockNote::WrongPassword, self.config.motion, out)), delay)
                }
                PamOutcome::Refused(r) => (P::Locked(s.refused(LockNote::Refused(r), self.config.motion, out)), delay),
            },
            (P::Locked(mut s), Event::ShakeTick) => {
                s.tick(out);
                (P::Locked(s), delay)
            }
            (P::Checking(mut s), Event::ShakeTick) => {
                s.tick(out);
                (P::Checking(s), delay)
            }
            (p, Event::SessionStarted | Event::LogindAppeared) => match delay {
                Delay::NotHeld(n) if want => (p, Delay::Held(n.take(out))),
                d => (p, d),
            },
            (p, Event::PrepareForSleep) if want => match p {
                P::Unlocked(s) => {
                    let l = s.lock(self.outputs, out);
                    (P::Locking(l), hold(delay))
                }
                P::Locking(s) => (P::Locking(s), hold(delay)),
                p @ (P::Locked(_) | P::Checking(_)) => match delay {
                    Delay::Held(fd) => (p, Delay::NotHeld(fd.release(out))),
                    d => (p, d),
                },
            },
            (p, Event::Resumed) => match delay {
                Delay::SleepPending(s) => (p, Delay::Held(s.resumed())),
                Delay::NotHeld(n) if want => (p, Delay::Held(n.take(out))),
                d => (p, d),
            },
            (p, Event::LogindVanished) => match delay {
                Delay::Held(fd) => (p, Delay::NotHeld(fd.lost())),
                Delay::SleepPending(s) => (p, Delay::NotHeld(s.lost())),
                d => (p, d),
            },
            (p, _) => (p, delay),
        };
        self.phase = phase;
        self.delay = delay;
    }

    fn state(&self) -> StateName {
        StateName {
            lock: match self.phase {
                Phase::Unlocked(_) => LockPhase::Unlocked,
                Phase::Locking(_) => LockPhase::Locking,
                Phase::Locked(_) => LockPhase::Locked,
                Phase::Checking(_) => LockPhase::Checking,
            },
            delay: match self.delay {
                Delay::NotHeld(_) => DelayPhase::NotHeld,
                Delay::Held(_) => DelayPhase::Held,
                Delay::SleepPending(_) => DelayPhase::SleepPending,
            },
        }
    }
}

fn hold(delay: Delay) -> Delay {
    match delay {
        Delay::Held(fd) => Delay::SleepPending(fd.hold_for_sleep()),
        d => d,
    }
}
