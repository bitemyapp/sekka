//! `lock` with rust-fsm 0.8.0: one table per orthogonal region (`screen`
//! for the lock screen, `delay` for logind's sleep delay); the outputs,
//! the field, the shake and the check counter live beside them in [`Data`].
//!
//! What the DSL can't hold, and where it went:
//!
//! - orthogonal regions: two tables; `handle` gives each event to the
//!   `screen` table, then to the `delay` table;
//! - the region crossings: entering `Locked` and leaving the lock screen
//!   resolve a pending sleep, so those actions feed `Settled` into the
//!   `delay` table; `PrepareForSleep`'s delay row depends on the screen
//!   region's state, so [`Lock::delay_input`] reads the `screen` table (an
//!   `In(state)` guard);
//! - the `Lockscreen ⊃ Locked ⊃ Checking` hierarchy: flat leaves, with the
//!   `Lockscreen` rows (`LockEnded`, `UnlockRequested`, outputs) repeated;
//! - entry and exit actions: the transition's own output runs them; `shake`
//!   is reset on leaving so that it is `> 0` only in `Locked`/`Checking`;
//! - the tables' states are `spec::lock::LockPhase` and `DelayPhase` (the
//!   crate's custom state type), so `state()` reads the tables directly;
//! - guards: `has_text`, the current check, `shake > 0`, a new or present
//!   output, and `lock_before_sleep` are decided by the input functions.

use rust_fsm::state_machine;
use spec::Sink;
use spec::lock::{Config, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

pub const IMPLEMENTED: bool = true;

state_machine! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[state_machine(state(spec::lock::LockPhase))]
    pub screen(Unlocked)

    Unlocked => {
        Lock => Locking [EnterLockscreen],
        Sleep => Locking [EnterLockscreen],
        OutputAdded => Unlocked [AddOutput],
        OutputRemoved => Unlocked [RemoveOutput],
    },
    Locking => {
        LockConfirmed => Locked [EnterLocked],
        LockEnded => Unlocked [LeaveEnded],
        UnlockRequested => Unlocked [LeaveReleased],
        OutputAdded => Locking [OpenSurface],
        OutputRemoved => Locking [CloseSurface],
    },
    Locked => {
        LockEnded => Unlocked [LeaveEnded],
        UnlockRequested => Unlocked [LeaveReleased],
        OutputAdded => Locked [OpenSurface],
        OutputRemoved => Locked [CloseSurface],
        Typed => Locked [Type],
        Escape => Locked [Clear],
        Submit => Checking [StartCheck],
        ShakeTick => Locked [Shake],
    },
    Checking => {
        LockEnded => Unlocked [LeaveEnded],
        UnlockRequested => Unlocked [LeaveReleased],
        OutputAdded => Checking [OpenSurface],
        OutputRemoved => Checking [CloseSurface],
        Accepted => Unlocked [LeaveReleased],
        WrongPassword => Locked [Refused],
        Refused => Locked [Refused],
        ShakeTick => Checking [Shake],
    },
}

state_machine! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[state_machine(state(spec::lock::DelayPhase))]
    pub delay(NotHeld)

    NotHeld => {
        Take => Held [TakeDelay],
        Resumed => Held [TakeDelay],
    },
    Held => {
        Sleep => SleepPending,
        SleepLocked => NotHeld [ReleaseDelay],
        Vanished => NotHeld,
    },
    SleepPending => {
        Settled => NotHeld [ReleaseDelay],
        Resumed => Held,
        Vanished => NotHeld,
    },
}

/// The two tables and the data they can't hold, in one struct (so the
/// tables' bytes pack with the data). `has_text` is the lock screen's field;
/// `shake` is `> 0` only in `Locked`/`Checking`; `last_check` is
/// machine-wide, and is also the running check's id in `Checking`.
#[derive(Clone, Debug)]
pub struct Data {
    screen: screen::StateMachine,
    delay: delay::StateMachine,
    pub outputs: u32,
    pub last_check: u32,
    pub has_text: bool,
    pub shake: u8,
}

pub struct Lock {
    data: Data,
    config: Config,
}

fn bit(o: u8) -> u32 {
    1u32 << (o & 31)
}

impl Lock {
    /// The screen region's table.
    pub fn screen_table(&self) -> &screen::StateMachine {
        &self.data.screen
    }

    /// The delay region's table.
    pub fn delay_table(&self) -> &delay::StateMachine {
        &self.data.delay
    }

    /// The data beside the tables.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// The screen region's guards.
    fn screen_input(&self, event: &Event) -> Option<screen::Input> {
        use screen::Input as I;
        let d = &self.data;
        Some(match *event {
            Event::Lock => I::Lock,
            Event::PrepareForSleep if self.config.lock_before_sleep => I::Sleep,
            Event::LockConfirmed => I::LockConfirmed,
            Event::LockEnded => I::LockEnded,
            Event::UnlockRequested => I::UnlockRequested,
            Event::OutputAdded(o) if d.outputs & bit(o) == 0 => I::OutputAdded,
            Event::OutputRemoved(o) if d.outputs & bit(o) != 0 => I::OutputRemoved,
            Event::Typed { .. } => I::Typed,
            Event::Escape => I::Escape,
            Event::Submit if d.has_text => I::Submit,
            Event::PamDone { check, outcome } if check == d.last_check => match outcome {
                PamOutcome::Accepted => I::Accepted,
                PamOutcome::WrongPassword => I::WrongPassword,
                PamOutcome::Refused(_) => I::Refused,
            },
            Event::ShakeTick if d.shake > 0 => I::ShakeTick,
            _ => return None,
        })
    }

    /// The delay region's guards (`PrepareForSleep` reads the screen region).
    fn delay_input(&self, event: &Event) -> Option<delay::Input> {
        use delay::Input as I;
        if !self.config.lock_before_sleep {
            return None;
        }
        Some(match *event {
            Event::SessionStarted | Event::LogindAppeared => I::Take,
            Event::PrepareForSleep => match self.data.screen.state() {
                LockPhase::Unlocked | LockPhase::Locking => I::Sleep,
                LockPhase::Locked | LockPhase::Checking => I::SleepLocked,
            },
            Event::Resumed => I::Resumed,
            Event::LogindVanished => I::Vanished,
            _ => return None,
        })
    }

    /// The lock screen resolved a pending sleep (confirmed, or left).
    fn settle(&mut self, out: &mut impl Sink<Effect>) {
        if let Ok(Some(delay::Output::ReleaseDelay)) = self.data.delay.consume(&delay::Input::Settled) {
            out.push(Effect::ReleaseDelay);
        }
    }

    fn leave(&mut self, release: bool, out: &mut impl Sink<Effect>) {
        if self.data.shake > 0 {
            out.push(Effect::CancelShake);
            self.data.shake = 0;
        }
        if release {
            out.push(Effect::ReleaseLock);
        }
        out.push(Effect::LockedHint(false));
        self.settle(out);
    }

    /// The screen region's actions. Exhaustive on the output, so a new
    /// output is a compile error; arms that need a payload take it from
    /// their event (the table pairs every output with one kind of event).
    fn screen_act(&mut self, action: screen::Output, event: Event, out: &mut impl Sink<Effect>) {
        use screen::Output as O;
        let d = &mut self.data;
        match action {
            O::EnterLockscreen => {
                out.push(Effect::RequestLock);
                let mut m = d.outputs;
                while m != 0 {
                    out.push(Effect::OpenSurface(m.trailing_zeros() as u8));
                    m &= m - 1;
                }
                d.has_text = false;
            }
            O::EnterLocked => {
                out.push(Effect::LockedHint(true));
                d.shake = 0;
                self.settle(out);
            }
            O::LeaveEnded => self.leave(false, out),
            O::LeaveReleased => self.leave(true, out),
            O::AddOutput | O::OpenSurface => {
                let Event::OutputAdded(o) = event else { unreachable!() };
                d.outputs |= bit(o);
                if action == O::OpenSurface {
                    out.push(Effect::OpenSurface(o));
                }
            }
            O::RemoveOutput | O::CloseSurface => {
                let Event::OutputRemoved(o) = event else { unreachable!() };
                d.outputs &= !bit(o);
                if action == O::CloseSurface {
                    out.push(Effect::CloseSurface(o));
                }
            }
            O::Type => {
                let Event::Typed { empty } = event else { unreachable!() };
                d.has_text = !empty;
            }
            O::Clear => d.has_text = false,
            O::StartCheck => {
                d.last_check += 1;
                out.push(Effect::StartCheck { check: d.last_check });
                d.has_text = false;
            }
            O::Refused => {
                let Event::PamDone { outcome, .. } = event else { unreachable!() };
                out.push(Effect::ShowNote(match outcome {
                    PamOutcome::Refused(r) => LockNote::Refused(r),
                    _ => LockNote::WrongPassword,
                }));
                if self.config.motion {
                    d.shake = 1;
                    out.push(Effect::ShakeStep(1));
                    out.push(Effect::ArmShake);
                }
            }
            O::Shake => {
                d.shake += 1;
                if d.shake >= 9 {
                    d.shake = 0;
                    out.push(Effect::ShakeStep(0));
                } else {
                    out.push(Effect::ShakeStep(d.shake));
                    out.push(Effect::ArmShake);
                }
            }
        }
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        let (screen, delay) = (screen::StateMachine::new(), delay::StateMachine::new());
        let data = Data { screen, delay, outputs: config.outputs, last_check: 0, has_text: false, shake: 0 };
        Self { data, config }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        // `Err`: no row for this (state, input): the spec ignores the event.
        if let Some(input) = self.screen_input(&event)
            && let Ok(Some(action)) = self.data.screen.consume(&input)
        {
            self.screen_act(action, event, out);
        }
        if let Some(input) = self.delay_input(&event)
            && let Ok(Some(action)) = self.data.delay.consume(&input)
        {
            out.push(match action {
                delay::Output::TakeDelay => Effect::TakeDelay,
                delay::Output::ReleaseDelay => Effect::ReleaseDelay,
            });
        }
    }

    fn state(&self) -> StateName {
        StateName { lock: *self.data.screen.state(), delay: *self.data.delay.state() }
    }
}
