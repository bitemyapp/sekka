//! `lock` with statum 0.9.0: statum has no orthogonal regions or
//! superstates, so each region is its own `#[machine]`: `LockScreen` (the
//! lock region's leaves, with `Locked`'s and `Checking`'s data in sealed
//! payloads and tokens for the others, the outputs and the last check as
//! fields) and `Inhibitor` (logind's delay: `NotHeld`, `Held`,
//! `SleepPending`, tokens). A delay can only be taken when none is held,
//! and once held for a sleep it is a `SleepPending` whose `release` is
//! private: only confirming the lock and the ways out of the lock screen
//! (through `settle`) release it. Those transitions take
//! the delay region (`&mut inhibitor::SomeState`, since a transition can't
//! return a tuple). `handle` matches both regions' `SomeState`s.

use crate::{SEAL, Seal};
use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};
use statum::{machine, state, transition};

pub const IMPLEMENTED: bool = true;

tokens!(NotHeldTok, HeldTok, SleepPendingTok, UnlockedTok, LockingTok);

#[state]
#[derive(Debug)]
pub enum DelayState {
    NotHeld(NotHeldTok),
    Held(HeldTok),
    /// logind waits to sleep; the delay is held until the lock is confirmed.
    SleepPending(SleepPendingTok),
}

/// logind's sleep-delay inhibitor.
#[machine]
#[derive(Debug)]
pub struct Inhibitor<DelayState> {
    seal: Seal,
}

impl Inhibitor<NotHeld> {
    #[inline]
    pub fn new() -> Self {
        Inhibitor::<NotHeld>::builder().state_data(NotHeldTok(())).seal(SEAL).build()
    }
}

impl Default for Inhibitor<NotHeld> {
    fn default() -> Self {
        Self::new()
    }
}

#[transition]
impl Inhibitor<NotHeld> {
    pub fn take(self, out: &mut impl Sink<Effect>) -> Inhibitor<Held> {
        out.push(Effect::TakeDelay);
        self.transition_with(HeldTok(()))
    }
}

#[transition]
impl Inhibitor<Held> {
    /// Let logind sleep now.
    pub fn release(self, out: &mut impl Sink<Effect>) -> Inhibitor<NotHeld> {
        out.push(Effect::ReleaseDelay);
        self.transition_with(NotHeldTok(()))
    }
    pub fn hold_for_sleep(self) -> Inhibitor<SleepPending> {
        self.transition_with(SleepPendingTok(()))
    }
    /// logind went away with it.
    pub fn lost(self) -> Inhibitor<NotHeld> {
        self.transition_with(NotHeldTok(()))
    }
}

#[transition]
impl Inhibitor<SleepPending> {
    /// That sleep is over; the fd still holds for the next one.
    pub fn resumed(self) -> Inhibitor<Held> {
        self.transition_with(HeldTok(()))
    }
    pub fn lost(self) -> Inhibitor<NotHeld> {
        self.transition_with(NotHeldTok(()))
    }
    fn release(self, out: &mut impl Sink<Effect>) -> Inhibitor<NotHeld> {
        out.push(Effect::ReleaseDelay);
        self.transition_with(NotHeldTok(()))
    }
}

/// Leaving `Locking` (confirmed, or the lock screen ends): a pending sleep goes ahead.
fn settle(delay: &mut inhibitor::SomeState, out: &mut impl Sink<Effect>) {
    use inhibitor::SomeState as D;
    *delay = match std::mem::replace(delay, D::NotHeld(Inhibitor::new())) {
        D::SleepPending(p) => D::NotHeld(p.release(out)),
        d => d,
    };
}

#[derive(Debug)]
pub struct LockedData {
    has_text: bool,
    shake: u8,
}

#[derive(Debug)]
pub struct CheckingData {
    shake: u8,
    check: u32,
}

impl CheckingData {
    pub fn check(&self) -> u32 {
        self.check
    }
}

#[state]
#[derive(Debug)]
pub enum LockState {
    Unlocked(UnlockedTok),
    Locking(LockingTok),
    Locked(LockedData),
    Checking(CheckingData),
}

#[machine]
#[derive(Debug)]
pub struct LockScreen<LockState> {
    outputs: u32,
    last_check: u32,
    motion: bool,
    seal: Seal,
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

fn leave(shake: u8, release: bool, delay: &mut inhibitor::SomeState, out: &mut impl Sink<Effect>) {
    if shake > 0 {
        out.push(Effect::CancelShake);
    }
    if release {
        out.push(Effect::ReleaseLock);
    }
    out.push(Effect::LockedHint(false));
    settle(delay, out);
}

impl LockScreen<Unlocked> {
    #[inline]
    pub fn new(config: Config) -> Self {
        LockScreen::<Unlocked>::builder()
            .state_data(UnlockedTok(()))
            .outputs(config.outputs)
            .last_check(0)
            .motion(config.motion)
            .seal(SEAL)
            .build()
    }
}

impl<S: LockStateTrait> LockScreen<S> {
    /// Tracks an output (bit `o`): whether it is new (`add`) or was present.
    fn output(&mut self, o: u8, add: bool) -> bool {
        let bit = 1u32 << (o & 31);
        let changed = (self.outputs & bit == 0) == add;
        if changed {
            self.outputs ^= bit;
        }
        changed
    }
}

#[transition]
impl LockScreen<Unlocked> {
    pub fn lock(self, out: &mut impl Sink<Effect>) -> LockScreen<Locking> {
        out.push(Effect::RequestLock);
        let mut m = self.outputs;
        while m != 0 {
            out.push(Effect::OpenSurface(m.trailing_zeros() as u8));
            m &= m - 1;
        }
        self.transition_with(LockingTok(()))
    }
}

#[transition]
impl LockScreen<Locking> {
    pub fn confirm(self, delay: &mut inhibitor::SomeState, out: &mut impl Sink<Effect>) -> LockScreen<Locked> {
        out.push(Effect::LockedHint(true));
        settle(delay, out);
        self.transition_with(LockedData { has_text: false, shake: 0 })
    }
    pub fn leave(
        self,
        release: bool,
        delay: &mut inhibitor::SomeState,
        out: &mut impl Sink<Effect>,
    ) -> LockScreen<Unlocked> {
        leave(0, release, delay, out);
        self.transition_with(UnlockedTok(()))
    }
}

impl LockScreen<Locked> {
    pub fn typed(&mut self, empty: bool) {
        self.state_data.has_text = !empty;
    }
    pub fn tick(&mut self, out: &mut impl Sink<Effect>) {
        shake_tick(&mut self.state_data.shake, out);
    }
}

#[transition]
impl LockScreen<Locked> {
    pub fn submit(
        mut self,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<LockScreen<Checking>, LockScreen<Locked>> {
        if !self.state_data.has_text {
            return Err(self);
        }
        self.last_check += 1;
        out.push(Effect::StartCheck { check: self.last_check });
        let check = self.last_check;
        Ok(self.transition_map(|LockedData { shake, .. }| CheckingData { shake, check }))
    }
    pub fn leave(
        self,
        release: bool,
        delay: &mut inhibitor::SomeState,
        out: &mut impl Sink<Effect>,
    ) -> LockScreen<Unlocked> {
        leave(self.state_data.shake, release, delay, out);
        self.transition_with(UnlockedTok(()))
    }
}

impl LockScreen<Checking> {
    pub fn tick(&mut self, out: &mut impl Sink<Effect>) {
        shake_tick(&mut self.state_data.shake, out);
    }
}

#[transition]
impl LockScreen<Checking> {
    pub fn accepted(self, delay: &mut inhibitor::SomeState, out: &mut impl Sink<Effect>) -> LockScreen<Unlocked> {
        self.leave(true, delay, out)
    }
    pub fn refused(self, note: LockNote, out: &mut impl Sink<Effect>) -> LockScreen<Locked> {
        out.push(Effect::ShowNote(note));
        let shake = if self.motion {
            out.push(Effect::ShakeStep(1));
            out.push(Effect::ArmShake);
            1
        } else {
            0
        };
        self.transition_with(LockedData { has_text: false, shake })
    }
    pub fn leave(
        self,
        release: bool,
        delay: &mut inhibitor::SomeState,
        out: &mut impl Sink<Effect>,
    ) -> LockScreen<Unlocked> {
        leave(self.state_data.shake, release, delay, out);
        self.transition_with(UnlockedTok(()))
    }
}

fn region(delay: &mut Option<inhibitor::SomeState>) -> &mut inhibitor::SomeState {
    delay.as_mut().expect("a delay")
}

pub struct Lock {
    lock_before_sleep: bool,
    /// `None` only while `handle` has moved the state out (statum's builder
    /// makes no cheap placeholder).
    screen: Option<lock_screen::SomeState>,
    /// The same, for the delay region (the lock transitions settle it through
    /// a `&mut inhibitor::SomeState`, with a placeholder: `settle`).
    delay: Option<inhibitor::SomeState>,
}

impl Lock {
    pub fn typed(&self) -> (&lock_screen::SomeState, &inhibitor::SomeState) {
        (self.screen.as_ref().expect("a state"), self.delay.as_ref().expect("a delay"))
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        Self {
            lock_before_sleep: config.lock_before_sleep,
            screen: Some(lock_screen::SomeState::Unlocked(LockScreen::<Unlocked>::new(config))),
            delay: Some(inhibitor::SomeState::NotHeld(Inhibitor::new())),
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use inhibitor::SomeState as D;
        use lock_screen::SomeState as P;
        let want = self.lock_before_sleep;
        // The delay region alone.
        match event {
            Event::SessionStarted | Event::LogindAppeared | Event::Resumed | Event::LogindVanished => {
                let delay = self.delay.take().expect("a delay");
                self.delay = Some(match (delay, event) {
                    (D::NotHeld(n), Event::SessionStarted | Event::LogindAppeared | Event::Resumed) if want => {
                        D::Held(n.take(out))
                    }
                    (D::SleepPending(s), Event::Resumed) => D::Held(s.resumed()),
                    (D::Held(fd), Event::LogindVanished) => D::NotHeld(fd.lost()),
                    (D::SleepPending(s), Event::LogindVanished) => D::NotHeld(s.lost()),
                    (d, _) => d,
                });
                return;
            }
            _ => {}
        }
        let delay = &mut self.delay;
        let screen = self.screen.take().expect("a state");
        self.screen = Some(match (screen, event) {
            (P::Unlocked(s), Event::Lock) => P::Locking(s.lock(out)),
            (P::Locking(s), Event::LockConfirmed) => P::Locked(s.confirm(region(delay), out)),
            (p, Event::LockEnded | Event::UnlockRequested) => {
                let release = event == Event::UnlockRequested;
                P::Unlocked(match p {
                    P::Unlocked(s) => s,
                    P::Locking(s) => s.leave(release, region(delay), out),
                    P::Locked(s) => s.leave(release, region(delay), out),
                    P::Checking(s) => s.leave(release, region(delay), out),
                })
            }
            (mut p, Event::OutputAdded(o) | Event::OutputRemoved(o)) => {
                let add = matches!(event, Event::OutputAdded(_));
                let (changed, shown) = match &mut p {
                    P::Unlocked(s) => (s.output(o, add), false),
                    P::Locking(s) => (s.output(o, add), true),
                    P::Locked(s) => (s.output(o, add), true),
                    P::Checking(s) => (s.output(o, add), true),
                };
                if changed && shown {
                    out.push(if add { Effect::OpenSurface(o) } else { Effect::CloseSurface(o) });
                }
                p
            }
            (P::Locked(mut s), Event::Typed { empty }) => {
                s.typed(empty);
                P::Locked(s)
            }
            (P::Locked(mut s), Event::Escape) => {
                s.typed(true);
                P::Locked(s)
            }
            (P::Locked(s), Event::Submit) => match s.submit(out) {
                Ok(c) => P::Checking(c),
                Err(l) => P::Locked(l),
            },
            (P::Checking(s), Event::PamDone { check, outcome }) if check == s.state_data.check => match outcome {
                PamOutcome::Accepted => P::Unlocked(s.accepted(region(delay), out)),
                PamOutcome::WrongPassword => P::Locked(s.refused(LockNote::WrongPassword, out)),
                PamOutcome::Refused(r) => P::Locked(s.refused(LockNote::Refused(r), out)),
            },
            (P::Locked(mut s), Event::ShakeTick) => {
                s.tick(out);
                P::Locked(s)
            }
            (P::Checking(mut s), Event::ShakeTick) => {
                s.tick(out);
                P::Checking(s)
            }
            (p, Event::PrepareForSleep) if want => {
                let p = match p {
                    P::Unlocked(s) => P::Locking(s.lock(out)),
                    p => p,
                };
                *delay = Some(match (delay.take().expect("a delay"), &p) {
                    (D::Held(fd), P::Locking(_)) => D::SleepPending(fd.hold_for_sleep()),
                    (D::Held(fd), P::Locked(_) | P::Checking(_)) => D::NotHeld(fd.release(out)),
                    (d, _) => d,
                });
                p
            }
            (p, _) => p,
        });
    }

    fn state(&self) -> StateName {
        use inhibitor::SomeState as D;
        use lock_screen::SomeState as P;
        StateName {
            lock: match self.typed().0 {
                P::Unlocked(_) => LockPhase::Unlocked,
                P::Locking(_) => LockPhase::Locking,
                P::Locked(_) => LockPhase::Locked,
                P::Checking(_) => LockPhase::Checking,
            },
            delay: match self.typed().1 {
                D::NotHeld(_) => DelayPhase::NotHeld,
                D::Held(_) => DelayPhase::Held,
                D::SleepPending(_) => DelayPhase::SleepPending,
            },
        }
    }
}
