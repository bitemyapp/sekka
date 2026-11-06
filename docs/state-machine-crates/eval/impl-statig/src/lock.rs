//! `lock` with statig 0.4.1. statig has no orthogonal regions, so each
//! region is its own machine and `Lock::handle` dispatches every event to
//! the lock region, then to the delay region with the lock region's phase in
//! its context (the spec's guards on the other region: "PrepareForSleep
//! [Locked or Checking]", "lock confirmed or lock screen left").
//!
//! The lock region: `lockscreen ⊃ locked ⊃ {idle, checking}` and `locking`,
//! with the spec's entry and exit actions (`RequestLock` and the surfaces,
//! `LockedHint`, `CancelShake`). `shake` is `locked`'s state-local storage,
//! carried by its leaves; `has_text` is `idle`'s alone (the spec puts it on
//! `Lockscreen`, but only `Locked.Idle` reads or writes it). An action's
//! signature has no event, so `handle` routes it through the context
//! (`Cx::event`): `lockscreen`'s exit releases the lock unless `LockEnded`
//! (the compositor's own end) ran it.

use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};
use statig::blocking::InitializedStateMachine;
use statig::prelude::*;

use crate::Nowhere;

pub const IMPLEMENTED: bool = true;

/// The lock region's context: where effects go, and the event being handled
/// (`None` while initializing).
pub struct Cx<'a> {
    out: &'a mut dyn Sink<Effect>,
    event: Option<Event>,
}

impl Cx<'_> {
    #[inline]
    fn push(&mut self, effect: Effect) {
        self.out.push(effect);
    }
}

/// The lock region's shared storage.
pub struct Screen {
    motion: bool,
    lock_before_sleep: bool,
    outputs: u32,
    last_check: u32,
}

impl Screen {
    /// Adds output `o`: `true` if it is new.
    fn add(&mut self, o: u8) -> bool {
        let bit = 1u32 << (o & 31);
        let new = self.outputs & bit == 0;
        self.outputs |= bit;
        new
    }

    /// Removes output `o`: `true` if it was there.
    fn remove(&mut self, o: u8) -> bool {
        let bit = 1u32 << (o & 31);
        let present = self.outputs & bit != 0;
        self.outputs &= !bit;
        present
    }

    /// A refused password: the note, then the shake (restarting a running one).
    fn refused(&self, note: LockNote, shake: u8, out: &mut Cx<'_>) -> Outcome<State> {
        out.push(Effect::ShowNote(note));
        if !self.motion {
            return Transition(State::idle(shake));
        }
        out.push(Effect::ShakeStep(1));
        out.push(Effect::ArmShake);
        Transition(State::idle(1))
    }
}

#[state_machine(initial = "State::unlocked()", state(derive(Debug, Clone, Copy, PartialEq, Eq)))]
impl Screen {
    #[state]
    fn unlocked(&mut self, event: &Event) -> Outcome<State> {
        match *event {
            Event::Lock => Transition(State::locking()),
            Event::PrepareForSleep if self.lock_before_sleep => Transition(State::locking()),
            Event::OutputAdded(o) => {
                self.add(o);
                Handled
            }
            Event::OutputRemoved(o) => {
                self.remove(o);
                Handled
            }
            _ => Handled,
        }
    }

    /// The lock screen shows: a surface on every output.
    #[superstate(entry_action = "request_lock", exit_action = "leave_lockscreen")]
    fn lockscreen(&mut self, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match *event {
            Event::LockEnded | Event::UnlockRequested => Transition(State::unlocked()),
            Event::OutputAdded(o) => {
                if self.add(o) {
                    context.push(Effect::OpenSurface(o));
                }
                Handled
            }
            Event::OutputRemoved(o) => {
                if self.remove(o) {
                    context.push(Effect::CloseSurface(o));
                }
                Handled
            }
            _ => Handled,
        }
    }

    /// enter Lockscreen.
    #[action]
    fn request_lock(&self, context: &mut Cx<'_>) {
        context.push(Effect::RequestLock);
        let mut m = self.outputs;
        while m != 0 {
            context.push(Effect::OpenSurface(m.trailing_zeros() as u8));
            m &= m - 1;
        }
    }

    /// leave Lockscreen, after `locked`'s exit cancelled the shake. After
    /// `LockEnded` the compositor already ended the lock: nothing to release.
    #[action]
    fn leave_lockscreen(context: &mut Cx<'_>) {
        if !matches!(context.event, Some(Event::LockEnded)) {
            context.push(Effect::ReleaseLock);
        }
        context.push(Effect::LockedHint(false));
    }

    /// Asked the compositor; typing is ignored until it confirms.
    #[state(superstate = "lockscreen")]
    fn locking(event: &Event) -> Outcome<State> {
        match event {
            Event::LockConfirmed => Transition(State::idle(0)),
            _ => Super,
        }
    }

    /// Locked: the shake runs while `shake > 0`.
    #[superstate(superstate = "lockscreen", entry_action = "locked_hint", exit_action = "cancel_shake")]
    fn locked(shake: &mut u8, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match event {
            Event::ShakeTick if *shake > 0 => {
                *shake += 1;
                if *shake >= 9 {
                    *shake = 0;
                    context.push(Effect::ShakeStep(0));
                } else {
                    context.push(Effect::ShakeStep(*shake));
                    context.push(Effect::ArmShake);
                }
                Handled
            }
            _ => Super,
        }
    }

    /// enter Locked (from `locking`; `shake` starts at 0).
    #[action]
    fn locked_hint(context: &mut Cx<'_>) {
        context.push(Effect::LockedHint(true));
    }

    #[action]
    fn cancel_shake(shake: &mut u8, context: &mut Cx<'_>) {
        if *shake > 0 {
            context.push(Effect::CancelShake);
        }
    }

    /// Locked, the field takes input.
    #[state(superstate = "locked")]
    fn idle(
        &mut self,
        #[default] has_text: &mut bool,
        shake: &mut u8,
        context: &mut Cx<'_>,
        event: &Event,
    ) -> Outcome<State> {
        match *event {
            Event::Typed { empty } => {
                *has_text = !empty;
                Handled
            }
            Event::Escape => {
                *has_text = false;
                Handled
            }
            Event::Submit if *has_text => {
                self.last_check += 1;
                context.push(Effect::StartCheck { check: self.last_check });
                Transition(State::checking(*shake, self.last_check))
            }
            _ => Super,
        }
    }

    /// Locked, PAM check `check` running (the field was cleared).
    #[state(superstate = "locked")]
    fn checking(&self, shake: &mut u8, check: &mut u32, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match *event {
            Event::PamDone { check: c, outcome } if c == *check => match outcome {
                PamOutcome::Accepted => Transition(State::unlocked()),
                PamOutcome::WrongPassword => self.refused(LockNote::WrongPassword, *shake, context),
                PamOutcome::Refused(r) => self.refused(LockNote::Refused(r), *shake, context),
            },
            _ => Super,
        }
    }
}

/// The delay region: logind's sleep-delay inhibitor, as its own machine.
pub mod delay {
    use spec::Sink;
    use spec::lock::{Effect, Event, LockPhase};
    use statig::prelude::*;

    /// The delay region's context: where effects go, and the lock region's
    /// phase after it handled the same event.
    pub struct Cx<'a> {
        pub(super) out: &'a mut dyn Sink<Effect>,
        pub(super) lock: LockPhase,
    }

    /// The delay region's shared storage.
    pub struct Inhibitor {
        pub(super) lock_before_sleep: bool,
    }

    #[state_machine(initial = "State::not_held()", state(derive(Debug, Clone, Copy, PartialEq, Eq)))]
    impl Inhibitor {
        #[state]
        fn not_held(&self, event: &Event) -> Outcome<State> {
            match event {
                Event::SessionStarted | Event::LogindAppeared | Event::Resumed if self.lock_before_sleep => {
                    Transition(State::held())
                }
                _ => Handled,
            }
        }

        /// The inhibitor fd is open.
        #[superstate(entry_action = "take_delay")]
        fn holding(event: &Event) -> Outcome<State> {
            match event {
                // The fd went with logind.
                Event::LogindVanished => Transition(State::not_held()),
                _ => Handled,
            }
        }

        #[action]
        fn take_delay(context: &mut Cx<'_>) {
            context.out.push(Effect::TakeDelay);
        }

        #[state(superstate = "holding")]
        fn held(&self, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
            match event {
                Event::PrepareForSleep if self.lock_before_sleep => match context.lock {
                    LockPhase::Locked | LockPhase::Checking => {
                        context.out.push(Effect::ReleaseDelay);
                        Transition(State::not_held())
                    }
                    // The lock region is locking now: hold the sleep until it's locked.
                    LockPhase::Unlocked | LockPhase::Locking => Transition(State::sleep_pending()),
                },
                _ => Super,
            }
        }

        /// logind waits to sleep: the delay is held until the lock is confirmed.
        #[state(superstate = "holding")]
        fn sleep_pending(context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
            match event {
                // That sleep is over; the fd is still valid (SYB-12).
                Event::Resumed => Transition(State::held()),
                Event::LogindVanished => Super,
                // Lock confirmed, or the lock screen left.
                _ if context.lock != LockPhase::Locking => {
                    context.out.push(Effect::ReleaseDelay);
                    Transition(State::not_held())
                }
                _ => Super,
            }
        }
    }
}

pub struct Lock {
    screen: InitializedStateMachine<Screen>,
    delay: InitializedStateMachine<delay::Inhibitor>,
}

impl Lock {
    pub fn state_enum(&self) -> (&State, &delay::State) {
        (self.screen.state(), self.delay.state())
    }

    fn phase(&self) -> LockPhase {
        match self.screen.state() {
            State::Unlocked {} => LockPhase::Unlocked,
            State::Locking { .. } => LockPhase::Locking,
            State::Idle { .. } => LockPhase::Locked,
            State::Checking { .. } => LockPhase::Checking,
        }
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        let screen = Screen {
            motion: config.motion,
            lock_before_sleep: config.lock_before_sleep,
            outputs: config.outputs,
            last_check: 0,
        };
        let screen = screen.uninitialized_state_machine().init_with_context(&mut Cx { out: &mut Nowhere, event: None });
        let delay = delay::Inhibitor { lock_before_sleep: config.lock_before_sleep };
        let delay = delay
            .uninitialized_state_machine()
            .init_with_context(&mut delay::Cx { out: &mut Nowhere, lock: LockPhase::Unlocked });
        Self { screen, delay }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.screen.handle_with_context(&event, &mut Cx { out, event: Some(event) });
        let lock = self.phase();
        self.delay.handle_with_context(&event, &mut delay::Cx { out, lock });
    }

    fn state(&self) -> StateName {
        let delay = match self.delay.state() {
            delay::State::NotHeld {} => DelayPhase::NotHeld,
            delay::State::Held {} => DelayPhase::Held,
            delay::State::SleepPending {} => DelayPhase::SleepPending,
        };
        StateName { lock: self.phase(), delay }
    }
}
