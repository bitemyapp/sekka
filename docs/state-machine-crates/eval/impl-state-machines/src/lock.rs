//! `lock` with state-machines 0.34.0: one `state_machine!` per orthogonal
//! region. `Locker` nests `Lockscreen ⊃ Locked ⊃ Checking` with entry and
//! exit hooks and superstate data (the field, the shake); `Delay` is
//! logind's sleep delay. The crate's own regions (`regions:`,
//! `runtime::Parallel`) need its `runtime` feature, dispatch with `async`
//! and have no cross-region guards, so `Lock::handle` composes the two
//! wrappers: `sleep` carries the lock phase as its payload (the delay
//! region's guards read it), and the lock settling (confirmed, or the lock
//! screen left) is relayed to `Delay` as an event.

use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};
use state_machines::state_machine;

pub const IMPLEMENTED: bool = true;

pub use machine::{
    Checking, Delay, DelayCtx, DelayHeldSleepOutcome, Held, Idle, Lock, Locked, Locker, LockerCheckingPamDoneOutcome,
    Locking, Lockscreen, NotHeld, SleepPending, Unlocked,
};

/// `Lockscreen`'s data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Field {
    has_text: bool,
}

/// `Locked`'s data: the shake step (0: still; the timer runs while > 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shake(u8);

/// The lock region's context: what outlives a lock screen.
#[derive(Debug)]
pub struct LockerCtx {
    config: Config,
    outputs: u32,
    last_check: u32,
    out: Vec<Effect>,
}

impl LockerCtx {
    pub fn new(config: Config) -> Self {
        Self { config, outputs: config.outputs, last_check: 0, out: Vec::with_capacity(crate::out::CAPACITY) }
    }
}

/// The generated machines, their callbacks and the facade. The module is
/// private so that the generated `Dynamic*` wrapper (with its
/// `new_init_state`), its event and its state enums aren't exported.
mod machine {
    use super::*;
    use crate::out::drain;

    state_machine! {
        name: Locker,
        context: LockerCtx,
        dynamic: true,
        initial: Unlocked,
        states: [
            Unlocked,
            superstate Lockscreen(Field) {
                state Locking,
                superstate Locked(Shake) {
                    state Idle,
                    state Checking(u32),
                },
            },
        ],
        lifecycle: {
            Lockscreen { enter: [request_lock], exit: [hint_unlocked] },
            Locked { enter: [hint_locked] },
        },
        events {
            lock {
                transition: { from: Unlocked, to: Lockscreen }
            }
            prepare_for_sleep {
                transition: { from: Unlocked, to: Lockscreen, guards: [locks_before_sleep] }
            }
            lock_confirmed {
                transition: { from: Locking, to: Locked }
            }
            // The compositor already ended the lock: no `ReleaseLock`.
            lock_ended {
                transition: { from: Lockscreen, to: Unlocked, before: [stop_shake] }
            }
            unlock_requested {
                transition: { from: Lockscreen, to: Unlocked, before: [stop_shake, release_lock] }
            }
            output_added {
                payload: u8,
                transition: { from: Unlocked, internal: true, guards: [is_new], before: [add_output] }
                transition: { from: Lockscreen, internal: true, guards: [is_new], before: [add_output, open_surface] }
            }
            output_removed {
                payload: u8,
                transition: { from: Unlocked, internal: true, guards: [is_present], before: [remove_output] }
                transition: { from: Lockscreen, internal: true, guards: [is_present], before: [remove_output, close_surface] }
            }
            typed {
                payload: bool,
                transition: { from: Idle, internal: true, before: [take_text] }
            }
            escape {
                transition: { from: Idle, internal: true, before: [clear_field] }
            }
            submit {
                transition: { from: Idle, to: Checking, guards: [has_text], data: start_check, before: [clear_field] }
            }
            pam_done {
                payload: (u32, PamOutcome),
                guards: [current],
                branching: true,
                transition: { from: Checking, to: Unlocked, guards: [accepted], before: [stop_shake_for, release_lock_for] }
                transition: { from: Checking, to: Idle, fallback: true, before: [show_refusal, shake] }
            }
            shake_tick {
                transition: { from: Locked, internal: true, guards: [shaking], before: [tick] }
            }
        }
    }

    impl<S> Locker<S> {
        fn field(&mut self) -> &mut Field {
            self.__state_data_lockscreen.get_or_insert_default()
        }

        fn shake_step(&mut self) -> &mut Shake {
            self.__state_data_locked.get_or_insert_default()
        }

        // Hooks.

        /// Entering `Lockscreen`.
        fn request_lock(&mut self) {
            self.ctx.out.push(Effect::RequestLock);
            let mut m = self.ctx.outputs;
            while m != 0 {
                self.ctx.out.push(Effect::OpenSurface(m.trailing_zeros() as u8));
                m &= m - 1;
            }
        }

        /// Entering `Locked` (from `Locking`).
        fn hint_locked(&mut self) {
            self.ctx.out.push(Effect::LockedHint(true));
        }

        /// Leaving `Lockscreen`, after the transition's own callbacks.
        fn hint_unlocked(&mut self) {
            self.ctx.out.push(Effect::LockedHint(false));
        }

        // Guards.

        fn locks_before_sleep(&self, ctx: &LockerCtx) -> bool {
            ctx.config.lock_before_sleep
        }

        fn is_new(&self, ctx: &LockerCtx, &o: &u8) -> bool {
            ctx.outputs & (1 << (o & 31)) == 0
        }

        fn is_present(&self, ctx: &LockerCtx, &o: &u8) -> bool {
            ctx.outputs & (1 << (o & 31)) != 0
        }

        fn has_text(&self, _: &LockerCtx) -> bool {
            self.state_data_lockscreen().is_some_and(|f| f.has_text)
        }

        fn current(&self, _: &LockerCtx, &(check, _): &(u32, PamOutcome)) -> bool {
            self.state_data_checking() == Some(&check)
        }

        fn accepted(&self, _: &LockerCtx, &(_, outcome): &(u32, PamOutcome)) -> bool {
            outcome == PamOutcome::Accepted
        }

        fn shaking(&self, _: &LockerCtx) -> bool {
            self.state_data_locked().is_some_and(|s| s.0 > 0)
        }

        // Factories and callbacks.

        fn start_check(&mut self) -> u32 {
            self.ctx.last_check += 1;
            self.ctx.out.push(Effect::StartCheck { check: self.ctx.last_check });
            self.ctx.last_check
        }

        /// The shake timer runs only in `Locked`: leaving the lock screen stops it.
        fn stop_shake(&mut self) {
            if self.state_data_locked().is_some_and(|s| s.0 > 0) {
                self.ctx.out.push(Effect::CancelShake);
            }
        }

        fn release_lock(&mut self) {
            self.ctx.out.push(Effect::ReleaseLock);
        }

        // A callback's arity follows its event's payload.
        fn stop_shake_for<P>(&mut self, _: &P) {
            self.stop_shake();
        }

        fn release_lock_for<P>(&mut self, _: &P) {
            self.release_lock();
        }

        fn add_output(&mut self, &o: &u8) {
            self.ctx.outputs |= 1 << (o & 31);
        }

        fn open_surface(&mut self, &o: &u8) {
            self.ctx.out.push(Effect::OpenSurface(o));
        }

        fn remove_output(&mut self, &o: &u8) {
            self.ctx.outputs &= !(1 << (o & 31));
        }

        fn close_surface(&mut self, &o: &u8) {
            self.ctx.out.push(Effect::CloseSurface(o));
        }

        fn take_text(&mut self, &empty: &bool) {
            self.field().has_text = !empty;
        }

        fn clear_field(&mut self) {
            self.field().has_text = false;
        }

        /// PAM's reason, as it said it (SHB-12).
        fn show_refusal(&mut self, &(_, outcome): &(u32, PamOutcome)) {
            let note = match outcome {
                PamOutcome::Refused(r) => LockNote::Refused(r),
                PamOutcome::WrongPassword | PamOutcome::Accepted => LockNote::WrongPassword,
            };
            self.ctx.out.push(Effect::ShowNote(note));
        }

        fn shake(&mut self, _: &(u32, PamOutcome)) {
            if self.ctx.config.motion {
                self.shake_step().0 = 1;
                self.ctx.out.push(Effect::ShakeStep(1));
                self.ctx.out.push(Effect::ArmShake);
            }
        }

        fn tick(&mut self) {
            let shake = self.shake_step();
            shake.0 += 1;
            if shake.0 >= 9 {
                shake.0 = 0;
                self.ctx.out.push(Effect::ShakeStep(0));
            } else {
                let step = shake.0;
                self.ctx.out.push(Effect::ShakeStep(step));
                self.ctx.out.push(Effect::ArmShake);
            }
        }
    }

    /// The delay region's context.
    #[derive(Debug)]
    pub struct DelayCtx {
        lock_before_sleep: bool,
        out: Vec<Effect>,
    }

    impl DelayCtx {
        pub fn new(config: Config) -> Self {
            Self { lock_before_sleep: config.lock_before_sleep, out: Vec::with_capacity(crate::out::CAPACITY) }
        }
    }

    state_machine! {
        name: Delay,
        context: DelayCtx,
        dynamic: true,
        initial: NotHeld,
        states: [NotHeld, Held, SleepPending],
        events {
            // SessionStarted, LogindAppeared.
            acquire {
                transition: { from: NotHeld, to: Held, guards: [locks_before_sleep], before: [take_delay] }
            }
            // PrepareForSleep, with the lock region's phase after it handled it.
            sleep {
                payload: LockPhase,
                guards: [locks_before_sleep_for],
                branching: true,
                transition: { from: Held, to: SleepPending, guards: [still_locking] }
                transition: { from: Held, to: NotHeld, fallback: true, before: [release_delay_for] }
            }
            // That sleep is over (the fd is still valid); or take one.
            resumed {
                transition: { from: SleepPending, to: Held }
                transition: { from: NotHeld, to: Held, guards: [locks_before_sleep], before: [take_delay] }
            }
            logind_vanished {
                transition: { from: [Held, SleepPending], to: NotHeld }
            }
            // The lock was confirmed, or the lock screen left.
            lock_settled {
                transition: { from: SleepPending, to: NotHeld, before: [release_delay] }
            }
        }
    }

    impl<S> Delay<S> {
        fn locks_before_sleep(&self, ctx: &DelayCtx) -> bool {
            ctx.lock_before_sleep
        }

        fn locks_before_sleep_for(&self, ctx: &DelayCtx, _: &LockPhase) -> bool {
            ctx.lock_before_sleep
        }

        fn still_locking(&self, _: &DelayCtx, &phase: &LockPhase) -> bool {
            matches!(phase, LockPhase::Unlocked | LockPhase::Locking)
        }

        fn take_delay(&mut self) {
            self.ctx.out.push(Effect::TakeDelay);
        }

        fn release_delay(&mut self) {
            self.ctx.out.push(Effect::ReleaseDelay);
        }

        fn release_delay_for(&mut self, _: &LockPhase) {
            self.release_delay();
        }
    }

    /// The context inside the wrapper (`None` once a panicking callback
    /// poisoned it). `inner` and `AnyLockerState` are private generated
    /// items, visible here because the macro expanded in this module.
    impl DynamicLocker {
        fn ctx(&mut self) -> Option<&mut LockerCtx> {
            Some(match self.inner.as_mut()? {
                AnyLockerState::Unlocked(m) => &mut m.ctx,
                AnyLockerState::Locking(m) => &mut m.ctx,
                AnyLockerState::Idle(m) => &mut m.ctx,
                AnyLockerState::Checking(m) => &mut m.ctx,
            })
        }
    }

    impl<S> Locker<S> {
        /// Takes the effects the callbacks buffered, for driving the typed
        /// machine directly (the compile-fail probes do).
        pub fn take_effects(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.ctx.out)
        }
    }

    /// The context inside the wrapper (`None` once a panicking callback
    /// poisoned it). `inner` and `AnyDelayState` are private generated
    /// items, visible here because the macro expanded in this module.
    impl DynamicDelay {
        fn ctx(&mut self) -> Option<&mut DelayCtx> {
            Some(match self.inner.as_mut()? {
                AnyDelayState::NotHeld(m) => &mut m.ctx,
                AnyDelayState::Held(m) => &mut m.ctx,
                AnyDelayState::SleepPending(m) => &mut m.ctx,
            })
        }
    }

    impl<S> Delay<S> {
        /// Takes the effects the callbacks buffered, for driving the typed
        /// machine directly (the compile-fail probes do).
        pub fn take_effects(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.ctx.out)
        }
    }

    /// The two regions behind the `Machine` facade.
    pub struct Lock {
        locker: DynamicLocker,
        delay: DynamicDelay,
    }

    impl Lock {
        fn phase(&self) -> LockPhase {
            match self.locker.current_state() {
                LockerState::Unlocked => LockPhase::Unlocked,
                LockerState::Locking => LockPhase::Locking,
                LockerState::Idle => LockPhase::Locked,
                LockerState::Checking => LockPhase::Checking,
            }
        }

        /// One event to the lock region, its effects passed on; `false`: no
        /// transition from there, or its guards refused (ignored).
        fn dispatch_locker(&mut self, event: LockerEvent, out: &mut impl Sink<Effect>) -> bool {
            let handled = self.locker.handle(event).is_ok();
            if let Some(ctx) = self.locker.ctx() {
                drain(&mut ctx.out, out);
            }
            handled
        }

        fn dispatch_delay(&mut self, event: DelayEvent, out: &mut impl Sink<Effect>) {
            let _ = self.delay.handle(event);
            if let Some(ctx) = self.delay.ctx() {
                drain(&mut ctx.out, out);
            }
        }
    }

    impl Machine for Lock {
        fn new(config: Config) -> Self {
            let locker = DynamicLocker::new(LockerCtx::new(config));
            Self { locker, delay: DynamicDelay::new(DelayCtx::new(config)) }
        }

        fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
            let event = match event {
                Event::SessionStarted | Event::LogindAppeared => return self.dispatch_delay(DelayEvent::Acquire, out),
                Event::Resumed => return self.dispatch_delay(DelayEvent::Resumed, out),
                Event::LogindVanished => return self.dispatch_delay(DelayEvent::LogindVanished, out),
                Event::PrepareForSleep => {
                    self.dispatch_locker(LockerEvent::PrepareForSleep, out);
                    return self.dispatch_delay(DelayEvent::Sleep(self.phase()), out);
                }
                Event::Lock => LockerEvent::Lock,
                Event::UnlockRequested => LockerEvent::UnlockRequested,
                Event::LockConfirmed => LockerEvent::LockConfirmed,
                Event::LockEnded => LockerEvent::LockEnded,
                Event::OutputAdded(o) => LockerEvent::OutputAdded(o),
                Event::OutputRemoved(o) => LockerEvent::OutputRemoved(o),
                Event::Typed { empty } => LockerEvent::Typed(empty),
                Event::Escape => LockerEvent::Escape,
                Event::Submit => LockerEvent::Submit,
                Event::PamDone { check, outcome } => LockerEvent::PamDone((check, outcome)),
                Event::ShakeTick => LockerEvent::ShakeTick,
            };
            let was = self.phase();
            if self.dispatch_locker(event, out) {
                let now = self.phase();
                // Confirmed (left `Locking`), or the lock screen left.
                if now != was && (was == LockPhase::Locking || now == LockPhase::Unlocked) {
                    self.dispatch_delay(DelayEvent::LockSettled, out);
                }
            }
        }

        fn state(&self) -> StateName {
            let delay = match self.delay.current_state() {
                DelayState::NotHeld => DelayPhase::NotHeld,
                DelayState::Held => DelayPhase::Held,
                DelayState::SleepPending => DelayPhase::SleepPending,
            };
            StateName { lock: self.phase(), delay }
        }
    }
}
