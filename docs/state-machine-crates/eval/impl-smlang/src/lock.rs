//! `lock` with smlang 0.8.0. smlang has no orthogonal regions, so each
//! region is its own machine (`name: Lock`, `name: Delay`), and the delay
//! region's machine lives in the lock region's context: the lock region's
//! actions forward what the delay region must see (the lock confirmed or
//! the lock screen left, a sleep while locked or not) as events to it, at
//! the point in the effect order the spec gives. Events only the delay
//! region takes go to it straight from `handle`.
//!
//! `Lockscreen ⊃ Locked ⊃ Checking` is emulated: `Locked.Idle`'s data is
//! `Field { has_text, shake }`, `Checking`'s is `Check { shake, check }`
//! (`Locked`'s `shake` rides in both), and the superstates' rows are
//! repeated per substate. The entry/exit actions are written into the
//! transitions' actions: smlang's `on_entry_*`/`on_exit_*` hooks see neither
//! the state's data (`shake`) nor the sink, nor which transition fired
//! (`ReleaseLock` depends on it).

use smlang::statemachine;
use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

pub const IMPLEMENTED: bool = true;

mod delay {
    //! The delay region (private, like the lock region's `fsm`).
    use smlang::statemachine;
    use spec::Sink;
    use spec::lock::Effect;

    statemachine! {
        name: Delay,
        temporary_context: &mut impl Sink<Effect>,
        derive_states: [Clone, Copy, Debug],
        derive_events: [Clone, Copy, Debug],
        transitions: {
            // `Take`: `SessionStarted` or `LogindAppeared`.
            *NotHeld + Take [lock_before_sleep] / take = Held,
            NotHeld + Resumed [lock_before_sleep] / take = Held,
            SleepPending + Resumed = Held,
            Held | SleepPending + Vanished = NotHeld,
            // `Sleep(locked)`: `PrepareForSleep`, from the lock region.
            Held + Sleep(bool) [locked] / release_for_sleep = NotHeld,
            Held + Sleep(bool) = SleepPending,
            // The lock was confirmed or the lock screen left.
            SleepPending + Settled / release = NotHeld,
        }
    }

    pub struct Logind {
        pub(super) lock_before_sleep: bool,
    }

    impl DelayStateMachineContext for Logind {
        fn lock_before_sleep(&self, _: &mut impl Sink<Effect>) -> Result<bool, ()> {
            Ok(self.lock_before_sleep)
        }
        fn locked(&self, _: &mut impl Sink<Effect>, locked: &bool) -> Result<bool, ()> {
            Ok(*locked)
        }
        fn take(&mut self, out: &mut impl Sink<Effect>) -> Result<(), ()> {
            out.push(Effect::TakeDelay);
            Ok(())
        }
        fn release_for_sleep(&mut self, out: &mut impl Sink<Effect>, _: bool) -> Result<(), ()> {
            self.release(out)
        }
        fn release(&mut self, out: &mut impl Sink<Effect>) -> Result<(), ()> {
            out.push(Effect::ReleaseDelay);
            Ok(())
        }
    }
}

pub use delay::DelayStates;
use delay::{DelayEvents, DelayStateMachine, Logind};

/// `Locked.Idle`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub has_text: bool,
    pub shake: u8,
}

/// `Checking`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Check {
    pub shake: u8,
    pub check: u32,
}

pub type Pam = (u32, PamOutcome);

/// The lock region's generated machine, kept private: its `new_with_state`
/// and `context_mut` would let callers forge a state.
mod fsm {
    use super::*;

    statemachine! {
        name: Lock,
        temporary_context: &mut impl Sink<Effect>,
        derive_states: [Clone, Copy, Debug],
        derive_events: [Clone, Copy, Debug],
        transitions: {
            *Unlocked + Lock / enter_lockscreen = Locking,
            Unlocked + PrepareForSleep [lock_before_sleep] / lock_for_sleep = Locking,
            // Without `lock_before_sleep` no delay is ever held, so the delay
            // region ignores these `Sleep`s.
            Locking + PrepareForSleep / sleep_locking,
            Locked(Field) + PrepareForSleep / sleep_locked = _,
            Checking(Check) + PrepareForSleep / sleep_checking = _,
            Locking + LockConfirmed / confirm = Locked(Field),
            Locking + LockEnded / ended_locking = Unlocked,
            Locked(Field) + LockEnded / ended_locked = Unlocked,
            Checking(Check) + LockEnded / ended_checking = Unlocked,
            Locking + UnlockRequested / unlock_locking = Unlocked,
            Locked(Field) + UnlockRequested / unlock_locked = Unlocked,
            Checking(Check) + UnlockRequested / unlock_checking = Unlocked,
            Unlocked + OutputAdded(u8) / add_output,
            Locking + OutputAdded(u8) / add_surface,
            Locked(Field) + OutputAdded(u8) / add_surface_locked = _,
            Checking(Check) + OutputAdded(u8) / add_surface_checking = _,
            Unlocked + OutputRemoved(u8) / remove_output,
            Locking + OutputRemoved(u8) / remove_surface,
            Locked(Field) + OutputRemoved(u8) / remove_surface_locked = _,
            Checking(Check) + OutputRemoved(u8) / remove_surface_checking = _,
            Locked(Field) + Typed(bool) / typed = _,
            Locked(Field) + Escape / escape = _,
            Locked(Field) + Submit [has_text] / start_check = Checking(Check),
            Checking(Check) + PamDone(Pam) [checked && accepted] / unlock_accepted = Unlocked,
            Checking(Check) + PamDone(Pam) [checked] / refused = Locked(Field),
            Locked(Field) + ShakeTick [shaking] / shake_field = _,
            Checking(Check) + ShakeTick [shaking_check] / shake_check = _,
        }
    }
}

pub use fsm::{LockError, LockEvents, LockStates};
use fsm::{LockStateMachine, LockStateMachineContext};

/// The lock region's context: the machine-wide data, and the delay region.
pub struct Screen {
    config: Config,
    outputs: u32,
    last_check: u32,
    delay: DelayStateMachine<Logind>,
}

impl Screen {
    fn new(config: Config) -> Self {
        Self {
            config,
            outputs: config.outputs,
            last_check: 0,
            delay: DelayStateMachine::new(Logind { lock_before_sleep: config.lock_before_sleep }),
        }
    }

    pub fn outputs(&self) -> u32 {
        self.outputs
    }

    /// The delay region's state (only the lock region's actions and
    /// `Lock::handle` send it events).
    pub fn delay_state(&self) -> &DelayStates {
        self.delay.state()
    }

    /// enter Lockscreen.
    fn enter(&mut self, out: &mut impl Sink<Effect>) {
        out.push(Effect::RequestLock);
        let mut m = self.outputs;
        while m != 0 {
            let o = m.trailing_zeros() as u8;
            out.push(Effect::OpenSurface(o));
            m &= m - 1;
        }
    }

    /// leave Lockscreen (with the substate's `shake`, 0 from `Locking`).
    fn leave(&mut self, shake: u8, release: bool, out: &mut impl Sink<Effect>) -> R {
        if shake > 0 {
            out.push(Effect::CancelShake);
        }
        if release {
            out.push(Effect::ReleaseLock);
        }
        out.push(Effect::LockedHint(false));
        let _ = self.delay.process_event(out, DelayEvents::Settled);
        Ok(())
    }

    fn sleep(&mut self, locked: bool, out: &mut impl Sink<Effect>) {
        let _ = self.delay.process_event(out, DelayEvents::Sleep(locked));
    }

    /// Adds `o`; whether it is new.
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

    fn add_shown(&mut self, o: u8, out: &mut impl Sink<Effect>) {
        if self.add(o) {
            out.push(Effect::OpenSurface(o));
        }
    }

    fn remove_shown(&mut self, o: u8, out: &mut impl Sink<Effect>) {
        if self.remove(o) {
            out.push(Effect::CloseSurface(o));
        }
    }

    /// shake: the new `shake`.
    fn shake(&self, out: &mut impl Sink<Effect>) -> u8 {
        if self.config.motion {
            out.push(Effect::ShakeStep(1));
            out.push(Effect::ArmShake);
            1
        } else {
            0
        }
    }
}

fn tick(shake: u8, out: &mut impl Sink<Effect>) -> u8 {
    let shake = shake + 1;
    if shake >= 9 {
        out.push(Effect::ShakeStep(0));
        0
    } else {
        out.push(Effect::ShakeStep(shake));
        out.push(Effect::ArmShake);
        shake
    }
}

type R<T = ()> = Result<T, ()>;

impl LockStateMachineContext for Screen {
    fn lock_before_sleep(&self, _: &mut impl Sink<Effect>) -> R<bool> {
        Ok(self.config.lock_before_sleep)
    }
    fn has_text(&self, _: &mut impl Sink<Effect>, f: &Field) -> R<bool> {
        Ok(f.has_text)
    }
    fn checked(&self, _: &mut impl Sink<Effect>, c: &Check, p: &Pam) -> R<bool> {
        Ok(p.0 == c.check)
    }
    fn accepted(&self, _: &mut impl Sink<Effect>, _: &Check, p: &Pam) -> R<bool> {
        Ok(p.1 == PamOutcome::Accepted)
    }
    fn shaking(&self, _: &mut impl Sink<Effect>, f: &Field) -> R<bool> {
        Ok(f.shake > 0)
    }
    fn shaking_check(&self, _: &mut impl Sink<Effect>, c: &Check) -> R<bool> {
        Ok(c.shake > 0)
    }

    fn enter_lockscreen(&mut self, out: &mut impl Sink<Effect>) -> R {
        self.enter(out);
        Ok(())
    }
    fn lock_for_sleep(&mut self, out: &mut impl Sink<Effect>) -> R {
        self.enter(out);
        self.sleep(false, out);
        Ok(())
    }
    fn sleep_locking(&mut self, out: &mut impl Sink<Effect>) -> R {
        self.sleep(false, out);
        Ok(())
    }
    fn sleep_locked(&mut self, out: &mut impl Sink<Effect>, f: &Field) -> R<Field> {
        self.sleep(true, out);
        Ok(*f)
    }
    fn sleep_checking(&mut self, out: &mut impl Sink<Effect>, c: &Check) -> R<Check> {
        self.sleep(true, out);
        Ok(*c)
    }
    fn confirm(&mut self, out: &mut impl Sink<Effect>) -> R<Field> {
        out.push(Effect::LockedHint(true));
        let _ = self.delay.process_event(out, DelayEvents::Settled);
        Ok(Field { has_text: false, shake: 0 })
    }
    fn ended_locking(&mut self, out: &mut impl Sink<Effect>) -> R {
        self.leave(0, false, out)
    }
    fn ended_locked(&mut self, out: &mut impl Sink<Effect>, f: &Field) -> R {
        self.leave(f.shake, false, out)
    }
    fn ended_checking(&mut self, out: &mut impl Sink<Effect>, c: &Check) -> R {
        self.leave(c.shake, false, out)
    }
    fn unlock_locking(&mut self, out: &mut impl Sink<Effect>) -> R {
        self.leave(0, true, out)
    }
    fn unlock_locked(&mut self, out: &mut impl Sink<Effect>, f: &Field) -> R {
        self.leave(f.shake, true, out)
    }
    fn unlock_checking(&mut self, out: &mut impl Sink<Effect>, c: &Check) -> R {
        self.leave(c.shake, true, out)
    }
    fn add_output(&mut self, _: &mut impl Sink<Effect>, o: u8) -> R {
        self.add(o);
        Ok(())
    }
    fn add_surface(&mut self, out: &mut impl Sink<Effect>, o: u8) -> R {
        self.add_shown(o, out);
        Ok(())
    }
    fn add_surface_locked(&mut self, out: &mut impl Sink<Effect>, f: &Field, o: u8) -> R<Field> {
        self.add_shown(o, out);
        Ok(*f)
    }
    fn add_surface_checking(&mut self, out: &mut impl Sink<Effect>, c: &Check, o: u8) -> R<Check> {
        self.add_shown(o, out);
        Ok(*c)
    }
    fn remove_output(&mut self, _: &mut impl Sink<Effect>, o: u8) -> R {
        self.remove(o);
        Ok(())
    }
    fn remove_surface(&mut self, out: &mut impl Sink<Effect>, o: u8) -> R {
        self.remove_shown(o, out);
        Ok(())
    }
    fn remove_surface_locked(&mut self, out: &mut impl Sink<Effect>, f: &Field, o: u8) -> R<Field> {
        self.remove_shown(o, out);
        Ok(*f)
    }
    fn remove_surface_checking(&mut self, out: &mut impl Sink<Effect>, c: &Check, o: u8) -> R<Check> {
        self.remove_shown(o, out);
        Ok(*c)
    }
    fn typed(&mut self, _: &mut impl Sink<Effect>, f: &Field, empty: bool) -> R<Field> {
        Ok(Field { has_text: !empty, ..*f })
    }
    fn escape(&mut self, _: &mut impl Sink<Effect>, f: &Field) -> R<Field> {
        Ok(Field { has_text: false, ..*f })
    }
    fn start_check(&mut self, out: &mut impl Sink<Effect>, f: &Field) -> R<Check> {
        self.last_check += 1;
        out.push(Effect::StartCheck { check: self.last_check });
        Ok(Check { shake: f.shake, check: self.last_check })
    }
    fn unlock_accepted(&mut self, out: &mut impl Sink<Effect>, c: &Check, _: Pam) -> R {
        self.leave(c.shake, true, out)
    }
    fn refused(&mut self, out: &mut impl Sink<Effect>, _: &Check, (_, outcome): Pam) -> R<Field> {
        let note = match outcome {
            PamOutcome::Refused(r) => LockNote::Refused(r),
            PamOutcome::WrongPassword | PamOutcome::Accepted => LockNote::WrongPassword,
        };
        out.push(Effect::ShowNote(note));
        Ok(Field { has_text: false, shake: self.shake(out) })
    }
    fn shake_field(&mut self, out: &mut impl Sink<Effect>, f: &Field) -> R<Field> {
        Ok(Field { shake: tick(f.shake, out), ..*f })
    }
    fn shake_check(&mut self, out: &mut impl Sink<Effect>, c: &Check) -> R<Check> {
        Ok(Check { shake: tick(c.shake, out), ..*c })
    }
}

pub struct Lock {
    sm: LockStateMachine<Screen>,
}

impl Lock {
    /// The lock region's generated `process_event`: the typed transition API.
    pub fn process_event(&mut self, out: &mut impl Sink<Effect>, event: LockEvents) -> Result<&LockStates, LockError> {
        self.sm.process_event(out, event)
    }

    /// The lock region's generated state.
    pub fn state_enum(&self) -> &LockStates {
        self.sm.state()
    }

    /// The lock region's context, read-only (`context().delay_state()`).
    pub fn context(&self) -> &Screen {
        self.sm.context()
    }

    fn delay(&mut self, event: DelayEvents, out: &mut impl Sink<Effect>) {
        let _ = self.sm.context_mut().delay.process_event(out, event);
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        Self { sm: LockStateMachine::new(Screen::new(config)) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let event = match event {
            // The delay region's own events.
            Event::SessionStarted | Event::LogindAppeared => return self.delay(DelayEvents::Take, out),
            Event::Resumed => return self.delay(DelayEvents::Resumed, out),
            Event::LogindVanished => return self.delay(DelayEvents::Vanished, out),
            Event::Lock => LockEvents::Lock,
            Event::UnlockRequested => LockEvents::UnlockRequested,
            Event::LockConfirmed => LockEvents::LockConfirmed,
            Event::LockEnded => LockEvents::LockEnded,
            Event::OutputAdded(o) => LockEvents::OutputAdded(o),
            Event::OutputRemoved(o) => LockEvents::OutputRemoved(o),
            Event::Typed { empty } => LockEvents::Typed(empty),
            Event::Escape => LockEvents::Escape,
            Event::Submit => LockEvents::Submit,
            Event::PamDone { check, outcome } => LockEvents::PamDone((check, outcome)),
            Event::ShakeTick => LockEvents::ShakeTick,
            Event::PrepareForSleep => LockEvents::PrepareForSleep,
        };
        // Undeclared pairs, stale checks and failed guards are `Err`: ignored.
        let _ = self.sm.process_event(out, event);
    }

    fn state(&self) -> StateName {
        let lock = match self.sm.state() {
            LockStates::Unlocked => LockPhase::Unlocked,
            LockStates::Locking => LockPhase::Locking,
            LockStates::Locked(_) => LockPhase::Locked,
            LockStates::Checking(_) => LockPhase::Checking,
        };
        let delay = match self.sm.context().delay.state() {
            DelayStates::NotHeld => DelayPhase::NotHeld,
            DelayStates::Held => DelayPhase::Held,
            DelayStates::SleepPending => DelayPhase::SleepPending,
        };
        StateName { lock, delay }
    }
}
