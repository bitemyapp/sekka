//! `lock` with finny 0.3.0: the top machine `Session` has two regions, the
//! lock (`Unlocked`, sub-machine `Lockscreen`) and the sleep delay
//! (`NotHeld`, `Held`, `SleepPending`). `Lockscreen` has `Locking` and the
//! sub-machine `Locked` (`Idle`, `Checking`). The superstates' entry and
//! exit actions are the parent level's actions on the sub-machine state;
//! `ShakeTick` (all of `Locked`) is `Lockscreen`'s internal transition on
//! `Locked`.
//!
//! What finny's sub-machines changed: a sub-machine sees only its own
//! context, built once from its parent's, so `has_text` lives in `Locked`
//! (the only level that reads it) and `last_check` there too (it outlives
//! the locks). An event forwarded into a sub-machine skips the other
//! regions, so `LockConfirmed` is a top-level event that the lock region
//! re-queues into `Lockscreen`. A sub-machine can't send its parent an
//! event, so the PAM acceptance that leaves `Lockscreen` is a top-level
//! transition whose guard looks two levels down. Each level keeps its own
//! effects; `handle` drains the innermost first (inner exits run before the
//! outer transition's actions; `Locked`'s entry comes before the delay
//! region's `ReleaseDelay` because both regions saw the event before the
//! queued confirmation ran).

use finny::decl::{BuiltFsm, FsmBuilder};
use finny::inspect::null::InspectNull;
use finny::{
    FsmCurrentState, FsmEventQueueArray, FsmEventQueueSender, FsmFactory, FsmResult, FsmTimersNull, finny_fsm,
};
use spec::Sink;
use spec::lock::{Config, DelayPhase, Effect, Event, LockNote, LockPhase, Machine, PamOutcome, StateName};

use crate::{Effects, Front};

pub const IMPLEMENTED: bool = true;

// Events of `Session`.
#[derive(Clone, Copy, Debug)]
pub struct SessionStarted;
#[derive(Clone, Copy, Debug)]
pub struct LockReq;
#[derive(Clone, Copy, Debug)]
pub struct UnlockRequested;
/// Also `Lockscreen`'s: the lock region re-queues it there.
#[derive(Clone, Copy, Debug)]
pub struct LockConfirmed;
#[derive(Clone, Copy, Debug)]
pub struct LockEnded;
#[derive(Clone, Copy, Debug)]
pub struct OutputAdded(pub u8);
#[derive(Clone, Copy, Debug)]
pub struct OutputRemoved(pub u8);
/// `PamDone { check, Accepted }`.
#[derive(Clone, Copy, Debug)]
pub struct Accepted {
    pub check: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct PrepareForSleep;
#[derive(Clone, Copy, Debug)]
pub struct Resumed;
#[derive(Clone, Copy, Debug)]
pub struct LogindVanished;
#[derive(Clone, Copy, Debug)]
pub struct LogindAppeared;

// Events of `Lockscreen`.
#[derive(Clone, Copy, Debug)]
pub struct ShakeTick;

// Events of `Locked`.
#[derive(Clone, Copy, Debug)]
pub struct Typed {
    pub empty: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Escape;
#[derive(Clone, Copy, Debug)]
pub struct Submit;
/// `PamDone { check, WrongPassword | Refused(r) }`.
#[derive(Clone, Copy, Debug)]
pub struct Refused {
    pub check: u32,
    pub note: LockNote,
}

// States.
#[derive(Default)]
pub struct Unlocked;
#[derive(Default)]
pub struct Locking;
#[derive(Default)]
pub struct Idle;
/// The PAM check running (`0` before the first).
#[derive(Default)]
pub struct Checking {
    pub check: u32,
}
#[derive(Default)]
pub struct NotHeld;
#[derive(Default)]
pub struct Held;
#[derive(Default)]
pub struct SleepPending;

/// `Session`'s context: the outputs (always tracked) and its effects.
pub struct Seat {
    config: Config,
    outputs: u32,
    out: Effects<Effect, 40>,
}

/// `Lockscreen`'s context: what `Locked`'s is built from.
pub struct Screen {
    motion: bool,
}

/// `Locked`'s context: the field, the shake, the machine-wide check counter
/// and the effects of `Locked` and its entry and exit.
pub struct Field {
    motion: bool,
    has_text: bool,
    shake: u8,
    last_check: u32,
    out: Effects<Effect, 8>,
}

impl Field {
    /// The shake's step (0: still; the timer runs while > 0).
    pub fn shake_step(&self) -> u8 {
        self.shake
    }

    fn shake(&mut self) {
        if self.motion {
            self.shake = 1;
            self.out.push(Effect::ShakeStep(1));
            self.out.push(Effect::ArmShake);
        }
    }

    fn tick(&mut self) {
        self.shake += 1;
        if self.shake >= 9 {
            self.shake = 0;
            self.out.push(Effect::ShakeStep(0));
        } else {
            self.out.push(Effect::ShakeStep(self.shake));
            self.out.push(Effect::ArmShake);
        }
    }
}

impl Locked {
    /// The PAM check in flight, if `Locked` is in `Checking`.
    pub fn check(&self) -> Option<u32> {
        (self.get_current_states()[0] == FsmCurrentState::State(LockedCurrentState::Checking))
            .then(|| self.get_state::<Checking>().check)
    }
}

impl Lockscreen {
    fn locked(&self) -> &Locked {
        self.get_state()
    }
}

#[finny_fsm]
fn locked_fsm(mut fsm: FsmBuilder<Locked, Field>) -> BuiltFsm {
    fsm.initial_state::<Idle>();

    fsm.state::<Idle>().on_event::<Typed>().internal_transition().action(|ev, ctx, _s| ctx.has_text = !ev.empty);
    fsm.state::<Idle>().on_event::<Escape>().internal_transition().action(|_ev, ctx, _s| ctx.has_text = false);
    fsm.state::<Idle>().on_event::<Submit>().transition_to::<Checking>().guard(|_ev, ctx, _s| ctx.has_text).action(
        |_ev, ctx, _from, to| {
            ctx.last_check += 1;
            to.check = ctx.last_check;
            ctx.out.push(Effect::StartCheck { check: to.check });
            ctx.has_text = false;
        },
    );
    fsm.state::<Checking>()
        .on_event::<Refused>()
        .transition_to::<Idle>()
        .guard(|ev, _ctx, s| ev.check == s.checking.check)
        .action(|ev, ctx, _from, _to| {
            ctx.out.push(Effect::ShowNote(ev.note));
            ctx.shake();
        });

    fsm.build()
}

#[finny_fsm]
fn lockscreen_fsm(mut fsm: FsmBuilder<Lockscreen, Screen>) -> BuiltFsm {
    fsm.initial_state::<Locking>();

    fsm.state::<Locking>().on_event::<LockConfirmed>().transition_to::<Locked>();
    // enter Locked; leave it (only by leaving the lock screen)
    fsm.sub_machine::<Locked>()
        .with_context(|screen| Field {
            motion: screen.motion,
            has_text: false,
            shake: 0,
            last_check: 0,
            out: Effects::new(Effect::ArmShake),
        })
        .on_entry(|locked, _ctx| {
            locked.context.has_text = false;
            locked.context.shake = 0;
            locked.context.out.push(Effect::LockedHint(true));
        })
        .on_exit(|locked, _ctx| {
            if locked.context.shake > 0 {
                locked.context.out.push(Effect::CancelShake);
            }
        });
    fsm.sub_machine::<Locked>()
        .on_event::<ShakeTick>()
        .internal_transition()
        .guard(|_ev, _ctx, s| s.locked.context.shake > 0)
        .action(|_ev, _ctx, locked| locked.context.tick());

    fsm.build()
}

#[finny_fsm]
fn session_fsm(mut fsm: FsmBuilder<Session, Seat>) -> BuiltFsm {
    fsm.initial_states::<(Unlocked, NotHeld)>();

    // The lock region.
    fsm.state::<Unlocked>().on_event::<LockReq>().transition_to::<Lockscreen>();
    fsm.state::<Unlocked>()
        .on_event::<PrepareForSleep>()
        .transition_to::<Lockscreen>()
        .guard(|_ev, ctx, _s| ctx.config.lock_before_sleep);
    fsm.state::<Unlocked>().on_event::<OutputAdded>().internal_transition().action(|ev, ctx, _s| {
        ctx.outputs |= 1 << (ev.0 & 31);
    });
    fsm.state::<Unlocked>().on_event::<OutputRemoved>().internal_transition().action(|ev, ctx, _s| {
        ctx.outputs &= !(1 << (ev.0 & 31));
    });

    // enter Lockscreen
    fsm.sub_machine::<Lockscreen>().with_context(|seat| Screen { motion: seat.config.motion }).on_entry(|_ls, ctx| {
        ctx.out.push(Effect::RequestLock);
        let mut m = ctx.outputs;
        while m != 0 {
            ctx.out.push(Effect::OpenSurface(m.trailing_zeros() as u8));
            m &= m - 1;
        }
    });
    // leave Lockscreen (its states' exits ran first: `CancelShake`)
    fsm.sub_machine::<Lockscreen>().on_event::<LockEnded>().transition_to::<Unlocked>().action(|_ev, ctx, _ls, _u| {
        ctx.out.push(Effect::LockedHint(false));
    });
    fsm.sub_machine::<Lockscreen>().on_event::<UnlockRequested>().transition_to::<Unlocked>().action(
        |_ev, ctx, _ls, _u| {
            ctx.out.push(Effect::ReleaseLock);
            ctx.out.push(Effect::LockedHint(false));
        },
    );
    fsm.sub_machine::<Lockscreen>()
        .on_event::<Accepted>()
        .transition_to::<Unlocked>()
        .guard(|ev, _ctx, s| s.lockscreen.locked().check() == Some(ev.check))
        .action(|_ev, ctx, _ls, _u| {
            ctx.out.push(Effect::ReleaseLock);
            ctx.out.push(Effect::LockedHint(false));
        });
    fsm.sub_machine::<Lockscreen>()
        .on_event::<LockConfirmed>()
        .internal_transition()
        .guard(|_ev, _ctx, s| {
            s.lockscreen.get_current_states()[0] == FsmCurrentState::State(LockscreenCurrentState::Locking)
        })
        .action(|_ev, ctx, _ls| {
            let _ = ctx.queue.enqueue(LockscreenEvents::from(LockConfirmed));
        });
    fsm.sub_machine::<Lockscreen>().on_event::<OutputAdded>().internal_transition().action(|ev, ctx, _ls| {
        let bit = 1 << (ev.0 & 31);
        if ctx.outputs & bit == 0 {
            ctx.outputs |= bit;
            ctx.out.push(Effect::OpenSurface(ev.0));
        }
    });
    fsm.sub_machine::<Lockscreen>().on_event::<OutputRemoved>().internal_transition().action(|ev, ctx, _ls| {
        let bit = 1 << (ev.0 & 31);
        if ctx.outputs & bit != 0 {
            ctx.outputs &= !bit;
            ctx.out.push(Effect::CloseSurface(ev.0));
        }
    });

    // The delay region. A guard can't see the lock region's current state,
    // but it sees `Lockscreen`'s, which is stopped outside the lock screen.
    fsm.state::<NotHeld>()
        .on_event::<SessionStarted>()
        .transition_to::<Held>()
        .guard(|_ev, ctx, _s| ctx.config.lock_before_sleep)
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::TakeDelay));
    fsm.state::<NotHeld>()
        .on_event::<LogindAppeared>()
        .transition_to::<Held>()
        .guard(|_ev, ctx, _s| ctx.config.lock_before_sleep)
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::TakeDelay));
    fsm.state::<NotHeld>()
        .on_event::<Resumed>()
        .transition_to::<Held>()
        .guard(|_ev, ctx, _s| ctx.config.lock_before_sleep)
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::TakeDelay));
    fsm.state::<Held>()
        .on_event::<PrepareForSleep>()
        .transition_to::<NotHeld>()
        .guard(|_ev, _ctx, s| {
            s.lockscreen.get_current_states()[0] == FsmCurrentState::State(LockscreenCurrentState::Locked)
        })
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::ReleaseDelay));
    fsm.state::<Held>().on_event::<PrepareForSleep>().transition_to::<SleepPending>();
    fsm.state::<Held>().on_event::<LogindVanished>().transition_to::<NotHeld>();
    // The pending sleep ends with the confirmation, with the lock screen, or
    // with the sleep.
    fsm.state::<SleepPending>()
        .on_event::<LockConfirmed>()
        .transition_to::<NotHeld>()
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::ReleaseDelay));
    fsm.state::<SleepPending>()
        .on_event::<LockEnded>()
        .transition_to::<NotHeld>()
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::ReleaseDelay));
    fsm.state::<SleepPending>()
        .on_event::<UnlockRequested>()
        .transition_to::<NotHeld>()
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::ReleaseDelay));
    fsm.state::<SleepPending>().on_event::<Resumed>().transition_to::<Held>();
    fsm.state::<SleepPending>().on_event::<LogindVanished>().transition_to::<NotHeld>();

    fsm.build()
}

pub struct Lock {
    fsm: Front<Session, 1>,
}

impl Lock {
    pub fn fsm(&self) -> &Front<Session, 1> {
        &self.fsm
    }

    /// finny's `dispatch` of a typed event (any level's), then the effects,
    /// innermost first (see the module comment).
    pub fn dispatch(&mut self, event: impl Into<SessionEvents>, out: &mut impl Sink<Effect>) -> FsmResult<()> {
        let r = self.fsm.dispatch(event);
        let lockscreen: &mut Lockscreen = self.fsm.states.as_mut();
        let locked: &mut Locked = lockscreen.states.as_mut();
        locked.context.out.drain(out);
        self.fsm.context.out.drain(out);
        r
    }

    /// The sub-machine `Lockscreen`; it exists (stopped) while unlocked.
    pub fn lockscreen(&self) -> &Lockscreen {
        self.fsm.get_state()
    }
}

impl Machine for Lock {
    fn new(config: Config) -> Self {
        let seat = Seat { config, outputs: config.outputs, out: Effects::new(Effect::RequestLock) };
        let mut fsm = Session::new_with(seat, FsmEventQueueArray::new(), InspectNull::new(), FsmTimersNull)
            .expect("the states are built from defaults");
        fsm.start().expect("the initial states have no entry actions");
        Self { fsm }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let locked = |e: LockedEvents| -> SessionEvents { LockscreenEvents::from(e).into() };
        let ev: SessionEvents = match event {
            Event::SessionStarted => SessionStarted.into(),
            Event::Lock => LockReq.into(),
            Event::UnlockRequested => UnlockRequested.into(),
            Event::LockConfirmed => LockConfirmed.into(),
            Event::LockEnded => LockEnded.into(),
            Event::OutputAdded(o) => OutputAdded(o).into(),
            Event::OutputRemoved(o) => OutputRemoved(o).into(),
            Event::Typed { empty } => locked(Typed { empty }.into()),
            Event::Escape => locked(Escape.into()),
            Event::Submit => locked(Submit.into()),
            // Leaving the lock screen is `Session`'s; a refusal is `Locked`'s.
            Event::PamDone { check, outcome: PamOutcome::Accepted } => Accepted { check }.into(),
            Event::PamDone { check, outcome: PamOutcome::WrongPassword } => {
                locked(Refused { check, note: LockNote::WrongPassword }.into())
            }
            Event::PamDone { check, outcome: PamOutcome::Refused(r) } => {
                locked(Refused { check, note: LockNote::Refused(r) }.into())
            }
            Event::ShakeTick => LockscreenEvents::from(ShakeTick).into(),
            Event::PrepareForSleep => PrepareForSleep.into(),
            Event::Resumed => Resumed.into(),
            Event::LogindVanished => LogindVanished.into(),
            Event::LogindAppeared => LogindAppeared.into(),
        };
        // `Err(NoTransition)`: ignored in both regions.
        let _ = self.dispatch(ev, out);
    }

    fn state(&self) -> StateName {
        use FsmCurrentState::State;
        let [lock, delay] = self.fsm.get_current_states();
        let lock = match lock {
            State(SessionCurrentState::Lockscreen) => match self.lockscreen().get_current_states()[0] {
                State(LockscreenCurrentState::Locked) => match self.lockscreen().locked().get_current_states()[0] {
                    State(LockedCurrentState::Checking) => LockPhase::Checking,
                    _ => LockPhase::Locked,
                },
                _ => LockPhase::Locking,
            },
            _ => LockPhase::Unlocked,
        };
        let delay = match delay {
            State(SessionCurrentState::Held) => DelayPhase::Held,
            State(SessionCurrentState::SleepPending) => DelayPhase::SleepPending,
            _ => DelayPhase::NotHeld,
        };
        StateName { lock, delay }
    }
}
