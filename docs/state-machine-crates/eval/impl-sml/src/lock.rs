//! `lock` with stateforward-sml 2.0.0: the root table has two orthogonal
//! regions (the lock screen and logind's delay); `Lockscreen` and `Locked`
//! are nested child tables. Entering `Lockscreen` requests the lock and
//! opens the surfaces, entering `Locked` sends the hint and leaving it stops
//! the shake (entry and exit actions). A PAM acceptance reaches `X` in
//! `Locked`, and the completions of `Locked` and `Lockscreen` release the
//! lock on the way out, so the effects keep the spec's order. The delay
//! region learns which phase a sleep found the lock in from an event the
//! lock region processes (`process(...)`). The superstates' data (the
//! field, the shake) and the outputs are the context; the running check is
//! `Checking`'s payload. Effects go through the per-call temporary context.

use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

pub const IMPLEMENTED: bool = true;

pub type Out<'a> = &'a mut dyn Sink<Effect>;

#[derive(Clone, Copy)]
pub struct PamDone {
    pub check: u32,
    pub outcome: PamOutcome,
}

/// Internal events the lock region processes to tell the delay region which
/// phase a sleep found. The fields are private: outside code can't build
/// them.
pub struct SleepWhileLocking(());
pub struct SleepWhileLocked(());

sml::sml! {
    Locked[temporary_context: &mut dyn Sink<Effect>] {
        *Idle + Typed(bool) / type_text,
         Idle + Escape / clear_text,
         Idle + Submit [has_text] / start_check = Checking(u32),
         Checking(u32) + PamDone(PamDone) [accepted] = X,
         Checking(u32) + PamDone(PamDone) [refused] / refuse = Idle,
    }

    Lockscreen[temporary_context: &mut dyn Sink<Effect>] {
        *Locking + LockConfirmed = state<Locked>,
         Locking + PrepareForSleep [lock_before_sleep] / process(SleepWhileLocking(())),
         state<Locked> + on_entry<_> / enter_locked,
         state<Locked> + on_exit<_> / exit_locked,
         state<Locked> + PrepareForSleep [lock_before_sleep] / process(SleepWhileLocked(())),
         state<Locked> + ShakeTick [shaking] / shake_step,
         state<Locked> / release_lock = X,
    }

    Lock[temporary_context: &mut dyn Sink<Effect>] {
        *Unlocked + Lock = state<Lockscreen>,
         Unlocked + PrepareForSleep [lock_before_sleep] / process(SleepWhileLocking(())) = state<Lockscreen>,
         Unlocked + OutputAdded(u8) / add_output,
         Unlocked + OutputRemoved(u8) / remove_output,
         state<Lockscreen> + on_entry<_> / enter_lockscreen,
         state<Lockscreen> + UnlockRequested / (release_lock, hint_unlocked) = Unlocked,
         state<Lockscreen> + LockEnded / hint_unlocked = Unlocked,
         state<Lockscreen> / hint_unlocked = Unlocked,
         state<Lockscreen> + OutputAdded(u8) / add_output_shown,
         state<Lockscreen> + OutputRemoved(u8) / remove_output_shown,

        *NotHeld + SessionStarted [lock_before_sleep] / take_delay = Held,
         NotHeld + LogindAppeared [lock_before_sleep] / take_delay = Held,
         NotHeld + Resumed [lock_before_sleep] / take_delay = Held,
         Held + event<SleepWhileLocking> = SleepPending,
         Held + event<SleepWhileLocked> / release_for_sleep = NotHeld,
         // Defensive: an untaken processed event makes the step that
         // processed it return `Err` (NOTES finding 3), though nothing was
         // left to do; these keep a taken `PrepareForSleep` `Ok`.
         NotHeld | SleepPending + event<SleepWhileLocking>,
         NotHeld | SleepPending + event<SleepWhileLocked>,
         Held | SleepPending + LogindVanished = NotHeld,
         SleepPending + Resumed = Held,
         SleepPending + LockConfirmed / release_delay = NotHeld,
         SleepPending + LockEnded / release_delay = NotHeld,
         SleepPending + UnlockRequested / release_delay = NotHeld,
    }
}

/// The context: the outputs, the check counter, and the data of
/// `Lockscreen` (the field) and `Locked` (the shake).
pub struct Shell {
    config: Config,
    outputs: u32,
    last_check: u32,
    has_text: bool,
    shake: u8,
}

type R = Result<(), ()>;
type G = Result<bool, ()>;

impl Shell {
    fn add(&mut self, o: u8) -> bool {
        let bit = 1u32 << (o & 31);
        let new = self.outputs & bit == 0;
        self.outputs |= bit;
        new
    }

    fn remove(&mut self, o: u8) -> bool {
        let bit = 1u32 << (o & 31);
        let present = self.outputs & bit != 0;
        self.outputs &= !bit;
        present
    }
}

impl LockStateMachineContext for Shell {
    fn type_text(&mut self, _: Out<'_>, empty: &bool) -> R {
        self.has_text = !*empty;
        Ok(())
    }

    fn clear_text(&mut self, _: Out<'_>) -> R {
        self.has_text = false;
        Ok(())
    }

    fn has_text(&self, _: Out<'_>) -> G {
        Ok(self.has_text)
    }

    fn start_check(&mut self, out: Out<'_>) -> Result<u32, ()> {
        self.last_check += 1;
        out.push(Effect::StartCheck { check: self.last_check });
        self.has_text = false;
        Ok(self.last_check)
    }

    fn accepted(&self, _: Out<'_>, check: &u32, e: &PamDone) -> G {
        Ok(e.check == *check && e.outcome == PamOutcome::Accepted)
    }

    fn refused(&self, _: Out<'_>, check: &u32, e: &PamDone) -> G {
        Ok(e.check == *check)
    }

    /// A wrong password or a refusal: say which, then shake.
    fn refuse(&mut self, out: Out<'_>, _: &u32, e: &PamDone) -> R {
        let note = match e.outcome {
            PamOutcome::Refused(r) => LockNote::Refused(r),
            PamOutcome::WrongPassword | PamOutcome::Accepted => LockNote::WrongPassword,
        };
        out.push(Effect::ShowNote(note));
        if self.config.motion {
            self.shake = 1;
            out.push(Effect::ShakeStep(1));
            out.push(Effect::ArmShake);
        }
        Ok(())
    }

    fn lock_before_sleep(&self, _: Out<'_>) -> G {
        Ok(self.config.lock_before_sleep)
    }

    fn enter_locked(&mut self, out: Out<'_>) -> R {
        out.push(Effect::LockedHint(true));
        self.shake = 0;
        Ok(())
    }

    fn exit_locked(&mut self, out: Out<'_>) -> R {
        if self.shake > 0 {
            out.push(Effect::CancelShake);
        }
        Ok(())
    }

    fn shaking(&self, _: Out<'_>) -> G {
        Ok(self.shake > 0)
    }

    fn shake_step(&mut self, out: Out<'_>) -> R {
        self.shake += 1;
        if self.shake >= 9 {
            self.shake = 0;
            out.push(Effect::ShakeStep(0));
        } else {
            out.push(Effect::ShakeStep(self.shake));
            out.push(Effect::ArmShake);
        }
        Ok(())
    }

    fn release_lock(&mut self, out: Out<'_>) -> R {
        out.push(Effect::ReleaseLock);
        Ok(())
    }

    fn enter_lockscreen(&mut self, out: Out<'_>) -> R {
        out.push(Effect::RequestLock);
        let mut m = self.outputs;
        while m != 0 {
            out.push(Effect::OpenSurface(m.trailing_zeros() as u8));
            m &= m - 1;
        }
        self.has_text = false;
        Ok(())
    }

    fn hint_unlocked(&mut self, out: Out<'_>) -> R {
        out.push(Effect::LockedHint(false));
        Ok(())
    }

    fn add_output(&mut self, _: Out<'_>, o: &u8) -> R {
        self.add(*o);
        Ok(())
    }

    fn remove_output(&mut self, _: Out<'_>, o: &u8) -> R {
        self.remove(*o);
        Ok(())
    }

    fn add_output_shown(&mut self, out: Out<'_>, o: &u8) -> R {
        if self.add(*o) {
            out.push(Effect::OpenSurface(*o));
        }
        Ok(())
    }

    fn remove_output_shown(&mut self, out: Out<'_>, o: &u8) -> R {
        if self.remove(*o) {
            out.push(Effect::CloseSurface(*o));
        }
        Ok(())
    }

    fn take_delay(&mut self, out: Out<'_>) -> R {
        out.push(Effect::TakeDelay);
        Ok(())
    }

    fn release_for_sleep(&mut self, out: Out<'_>, _: &SleepWhileLocked) -> R {
        out.push(Effect::ReleaseDelay);
        Ok(())
    }

    fn release_delay(&mut self, out: Out<'_>) -> R {
        out.push(Effect::ReleaseDelay);
        Ok(())
    }
}

pub struct Lock {
    /// The generated machine: its `process_event`, `is` and
    /// `visit_current_states` are the typed API.
    pub machine: LockStateMachine<Shell>,
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        let shell = Shell { config, outputs: config.outputs, last_check: 0, has_text: false, shake: 0 };
        Self { machine: LockStateMachine::new(shell) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use LockEvents as E;
        let event = match event {
            Event::SessionStarted => E::SessionStarted,
            Event::Lock => E::Lock,
            Event::UnlockRequested => E::UnlockRequested,
            Event::LockConfirmed => E::LockConfirmed,
            Event::LockEnded => E::LockEnded,
            Event::OutputAdded(o) => E::OutputAdded(o),
            Event::OutputRemoved(o) => E::OutputRemoved(o),
            Event::Typed { empty } => E::Typed(empty),
            Event::Escape => E::Escape,
            Event::Submit => E::Submit,
            Event::PamDone { check, outcome } => E::PamDone(PamDone { check, outcome }),
            Event::ShakeTick => E::ShakeTick,
            Event::PrepareForSleep => E::PrepareForSleep,
            Event::Resumed => E::Resumed,
            Event::LogindVanished => E::LogindVanished,
            Event::LogindAppeared => E::LogindAppeared,
        };
        // An event no region takes is `Err(InvalidEvent)`: ignored, as the spec says.
        let _ = self.machine.process_event(out, event);
    }

    fn state(&self) -> StateName {
        let m = &self.machine;
        let mut delay = DelayPhase::NotHeld;
        let mut lock = LockPhase::Unlocked;
        m.visit_current_states(|s| match s {
            LockStates::Unlocked => lock = LockPhase::Unlocked,
            LockStates::Lockscreen => {
                lock = if m.is(&LockLockscreenStates::Locking) {
                    LockPhase::Locking
                } else if m.is(&LockLockedStates::Checking(0)) {
                    LockPhase::Checking
                } else {
                    LockPhase::Locked
                }
            }
            LockStates::NotHeld => delay = DelayPhase::NotHeld,
            LockStates::Held => delay = DelayPhase::Held,
            LockStates::SleepPending => delay = DelayPhase::SleepPending,
        });
        StateName { lock, delay }
    }
}
