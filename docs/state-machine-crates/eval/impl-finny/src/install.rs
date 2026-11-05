//! `install` with finny 0.3.0: the top machine `Installer` has `Idle`,
//! `Prepared`, `Cancelling`, the terminal states, and two sub-machines,
//! `Preparing` (`Checks` … `Cache`) and `Executing` (`Erase` … `Flush`).
//! Each stage's entry action runs it; `Executing`'s entry and exit hold and
//! release the sleep inhibitor; the superstates' own transitions (`Cancel`,
//! `Confirm`, `StageFailed`) are `Installer`'s transitions on the
//! sub-machine state. A sub-machine can't leave itself, so the stage results
//! that end a sub-machine (`Evaluated` on the deferred route, `Built`
//! without room, `Cached`, the last `StepDone`) are `Installer`'s
//! transitions, guarded on the sub-machine's current state and data.
//! finny doesn't pass an event a sub-machine ignores up to its parent, so
//! `handle` does: a stage result goes to the sub-machine first, then (on
//! `Err(NoTransition)`) to `Installer`.

use finny::decl::{BuiltFsm, FsmBuilder};
use finny::inspect::null::InspectNull;
use finny::{FsmCurrentState, FsmEventQueueNull, FsmFactory, FsmResult, FsmTimersNull, finny_fsm};
use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

use crate::{Effects, Unqueued};

pub const IMPLEMENTED: bool = true;

// Events (the stage results are also the sub-machines' events).
#[derive(Clone, Copy, Debug)]
pub struct Begin;
#[derive(Clone, Copy, Debug)]
pub struct Checked {
    pub root_mib: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Configured;
#[derive(Clone, Copy, Debug)]
pub struct Evaluated {
    pub download_mib: u64,
    pub unpacked_mib: u64,
    pub budget_mib: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Built {
    pub closure_mib: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Cached;
#[derive(Clone, Copy, Debug)]
pub struct StepDone;
#[derive(Clone, Copy, Debug)]
pub struct StageFailed;
#[derive(Clone, Copy, Debug)]
pub struct Confirm {
    pub phrase_ok: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Cancel;

// States of `Installer`.
#[derive(Default)]
pub struct Idle;
#[derive(Default)]
pub struct Prepared {
    pub deferred: bool,
}
#[derive(Default)]
pub struct Cancelling;
#[derive(Default)]
pub struct Complete;
#[derive(Default)]
pub struct Failed;
#[derive(Default)]
pub struct Refused;
#[derive(Default)]
pub struct Cancelled;

// States of `Preparing`.
#[derive(Default)]
pub struct Checks;
#[derive(Default)]
pub struct Configuration {
    pub root_mib: u64,
}
#[derive(Default)]
pub struct Evaluation {
    pub root_mib: u64,
}
#[derive(Default)]
pub struct Build {
    pub root_mib: u64,
}
#[derive(Default)]
pub struct Cache {
    pub root_mib: u64,
    pub closure_mib: u64,
}

// States of `Executing`.
#[derive(Default)]
pub struct Erase {
    pub deferred: bool,
}
#[derive(Default)]
pub struct Format {
    pub deferred: bool,
}
#[derive(Default)]
pub struct Files {
    pub deferred: bool,
}
#[derive(Default)]
pub struct Copy;
#[derive(Default)]
pub struct BuildOnTarget;
#[derive(Default)]
pub struct Bootloader;
#[derive(Default)]
pub struct Record;
#[derive(Default)]
pub struct Flush;

/// The contexts: each level's effects (and the reserve, where it's read).
pub struct Host {
    reserve_mib: u64,
    out: Effects<Effect, 4>,
}
pub struct Prep {
    reserve_mib: u64,
    out: Effects<Effect, 2>,
}
pub struct Exec {
    out: Effects<Effect, 4>,
}

impl Prep {
    fn run(&mut self, stage: Stage) {
        self.out.push(Effect::Run(stage));
    }
}

impl Exec {
    fn step(&mut self, n: u8, stage: Stage) {
        self.out.push(Effect::Progress(n));
        self.out.push(Effect::Run(stage));
    }
}

impl Evaluated {
    fn deferred(&self) -> bool {
        self.download_mib > self.budget_mib
    }
}

impl Preparing {
    /// The stage's data, if `Preparing` is in that stage.
    fn at<S>(&self, kind: PreparingCurrentState) -> Option<&S>
    where
        PreparingStates: AsRef<S>,
    {
        (self.get_current_states()[0] == FsmCurrentState::State(kind)).then(|| self.get_state())
    }
}

#[finny_fsm]
fn preparing_fsm(mut fsm: FsmBuilder<Preparing, Prep>) -> BuiltFsm {
    fsm.initial_state::<Checks>();

    fsm.state::<Checks>().on_entry(|_s, ctx| ctx.run(Stage::Checks));
    fsm.state::<Configuration>().on_entry(|_s, ctx| ctx.run(Stage::Configuration));
    fsm.state::<Evaluation>().on_entry(|_s, ctx| ctx.run(Stage::Evaluation));
    fsm.state::<Build>().on_entry(|_s, ctx| ctx.run(Stage::Build));
    fsm.state::<Cache>().on_entry(|_s, ctx| ctx.run(Stage::Cache));

    fsm.state::<Checks>().on_event::<Checked>().transition_to::<Configuration>().action(|ev, _ctx, _from, to| {
        to.root_mib = ev.root_mib;
    });
    fsm.state::<Configuration>()
        .on_event::<Configured>()
        .transition_to::<Evaluation>()
        .action(|_ev, _ctx, from, to| to.root_mib = from.root_mib);
    // The in-memory route; the deferred one leaves `Preparing` (`Installer`).
    fsm.state::<Evaluation>()
        .on_event::<Evaluated>()
        .transition_to::<Build>()
        .guard(|ev, _ctx, _s| !ev.deferred())
        .action(|_ev, _ctx, from, to| to.root_mib = from.root_mib);
    fsm.state::<Build>()
        .on_event::<Built>()
        .transition_to::<Cache>()
        .guard(|ev, ctx, s| s.build.root_mib >= ev.closure_mib.saturating_add(ctx.reserve_mib))
        .action(|ev, _ctx, from, to| *to = Cache { root_mib: from.root_mib, closure_mib: ev.closure_mib });

    fsm.build()
}

#[finny_fsm]
fn executing_fsm(mut fsm: FsmBuilder<Executing, Exec>) -> BuiltFsm {
    fsm.initial_state::<Erase>();

    fsm.state::<Erase>().on_entry(|_s, ctx| ctx.step(1, Stage::Erase));
    fsm.state::<Format>().on_entry(|_s, ctx| ctx.step(2, Stage::Format));
    fsm.state::<Files>().on_entry(|_s, ctx| ctx.step(3, Stage::Files));
    fsm.state::<Copy>().on_entry(|_s, ctx| ctx.step(4, Stage::Copy));
    fsm.state::<BuildOnTarget>().on_entry(|_s, ctx| ctx.step(4, Stage::BuildOnTarget));
    fsm.state::<Bootloader>().on_entry(|_s, ctx| ctx.step(5, Stage::Bootloader));
    fsm.state::<Record>().on_entry(|_s, ctx| ctx.out.push(Effect::Run(Stage::Record)));
    fsm.state::<Flush>().on_entry(|_s, ctx| ctx.step(6, Stage::Flush));

    fsm.state::<Erase>().on_event::<StepDone>().transition_to::<Format>().action(|_ev, _ctx, from, to| {
        to.deferred = from.deferred;
    });
    fsm.state::<Format>().on_event::<StepDone>().transition_to::<Files>().action(|_ev, _ctx, from, to| {
        to.deferred = from.deferred;
    });
    fsm.state::<Files>().on_event::<StepDone>().transition_to::<BuildOnTarget>().guard(|_ev, _ctx, s| s.files.deferred);
    fsm.state::<Files>().on_event::<StepDone>().transition_to::<Copy>();
    fsm.state::<Copy>().on_event::<StepDone>().transition_to::<Bootloader>();
    fsm.state::<BuildOnTarget>().on_event::<StepDone>().transition_to::<Bootloader>();
    fsm.state::<Bootloader>().on_event::<StepDone>().transition_to::<Record>();
    fsm.state::<Record>().on_event::<StepDone>().transition_to::<Flush>();
    // A failed record is a warning.
    fsm.state::<Record>().on_event::<StageFailed>().transition_to::<Flush>().action(|_ev, ctx, _from, _to| {
        ctx.out.push(Effect::Warn(Warning::NoRecord));
    });

    fsm.build()
}

#[finny_fsm]
fn installer_fsm(mut fsm: FsmBuilder<Installer, Host>) -> BuiltFsm {
    fsm.initial_state::<Idle>();

    fsm.state::<Idle>().on_event::<Begin>().transition_to::<Preparing>();
    fsm.state::<Idle>().on_event::<Cancel>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });
    fsm.state::<Idle>().on_event::<Confirm>().transition_to::<Failed>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Failed { erased: false });
    });

    // Preparing as a whole.
    fsm.sub_machine::<Preparing>()
        .with_context(|host| Prep { reserve_mib: host.reserve_mib, out: Effects::new(Effect::Cancelled) });
    fsm.sub_machine::<Preparing>().on_event::<Cancel>().transition_to::<Cancelling>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Interrupt);
    });
    // A confirmation during preparation is a protocol error.
    fsm.sub_machine::<Preparing>().on_event::<Confirm>().transition_to::<Failed>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Interrupt);
        ctx.out.push(Effect::Failed { erased: false });
    });
    fsm.sub_machine::<Preparing>().on_event::<StageFailed>().transition_to::<Failed>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Failed { erased: false });
    });
    // The stage results that end the preparation.
    fsm.sub_machine::<Preparing>()
        .on_event::<Evaluated>()
        .transition_to::<Prepared>()
        .guard(|ev, ctx, s| {
            ev.deferred()
                && s.preparing
                    .at::<Evaluation>(PreparingCurrentState::Evaluation)
                    .is_some_and(|e| e.root_mib >= ev.unpacked_mib.saturating_add(ctx.reserve_mib))
        })
        .action(|_ev, ctx, _from, to| {
            ctx.out.push(Effect::Prepared { deferred: true, closure_mib: 0 });
            to.deferred = true;
        });
    fsm.sub_machine::<Preparing>()
        .on_event::<Evaluated>()
        .transition_to::<Refused>()
        .guard(|ev, _ctx, s| ev.deferred() && s.preparing.at::<Evaluation>(PreparingCurrentState::Evaluation).is_some())
        .action(|ev, ctx, from, _to| {
            let root_mib = from.get_state::<Evaluation>().root_mib;
            let needed_mib = ev.unpacked_mib.saturating_add(ctx.reserve_mib);
            ctx.out.push(Effect::Refused { needed_mib, root_mib });
        });
    fsm.sub_machine::<Preparing>()
        .on_event::<Built>()
        .transition_to::<Refused>()
        .guard(|_ev, _ctx, s| s.preparing.at::<Build>(PreparingCurrentState::Build).is_some())
        .action(|ev, ctx, from, _to| {
            let root_mib = from.get_state::<Build>().root_mib;
            let needed_mib = ev.closure_mib.saturating_add(ctx.reserve_mib);
            ctx.out.push(Effect::Refused { needed_mib, root_mib });
        });
    fsm.sub_machine::<Preparing>()
        .on_event::<Cached>()
        .transition_to::<Prepared>()
        .guard(|_ev, _ctx, s| s.preparing.at::<Cache>(PreparingCurrentState::Cache).is_some())
        .action(|_ev, ctx, from, to| {
            let closure_mib = from.get_state::<Cache>().closure_mib;
            ctx.out.push(Effect::Prepared { deferred: false, closure_mib });
            to.deferred = false;
        });

    fsm.state::<Prepared>()
        .on_event::<Confirm>()
        .transition_to::<Executing>()
        .guard(|ev, _ctx, _s| ev.phrase_ok)
        .action(|_ev, _ctx, from, to| {
            let erase: &mut Erase = to.states.as_mut();
            erase.deferred = from.deferred;
        });
    fsm.state::<Prepared>().on_event::<Confirm>().transition_to::<Failed>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Failed { erased: false });
    });
    fsm.state::<Prepared>().on_event::<Cancel>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });

    // A cancelled stage's result or failure.
    fsm.state::<Cancelling>().on_event::<Checked>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });
    fsm.state::<Cancelling>().on_event::<Configured>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });
    fsm.state::<Cancelling>().on_event::<Evaluated>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });
    fsm.state::<Cancelling>().on_event::<Built>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });
    fsm.state::<Cancelling>().on_event::<Cached>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });
    fsm.state::<Cancelling>().on_event::<StepDone>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });
    fsm.state::<Cancelling>().on_event::<StageFailed>().transition_to::<Cancelled>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Cancelled);
    });

    // Executing as a whole: the sleep inhibitor covers it; `Cancel` is ignored.
    fsm.sub_machine::<Executing>()
        .with_context(|_host| Exec { out: Effects::new(Effect::Complete) })
        .on_entry(|_exec, ctx| ctx.out.push(Effect::StayAwake))
        .on_exit(|_exec, ctx| ctx.out.push(Effect::AllowSleep));
    fsm.sub_machine::<Executing>()
        .on_event::<StepDone>()
        .transition_to::<Complete>()
        .guard(|_ev, _ctx, s| {
            s.executing.get_current_states()[0] == FsmCurrentState::State(ExecutingCurrentState::Flush)
        })
        .action(|_ev, ctx, _f, _t| ctx.out.push(Effect::Complete));
    // Any step but `Record` (which handles its own failure).
    fsm.sub_machine::<Executing>().on_event::<StageFailed>().transition_to::<Failed>().action(|_ev, ctx, _f, _t| {
        ctx.out.push(Effect::Failed { erased: true });
    });

    // The terminal states ignore everything (a target must be declared too).
    fsm.state::<Complete>();
    fsm.state::<Failed>();
    fsm.state::<Refused>();
    fsm.state::<Cancelled>();

    fsm.build()
}

pub struct Install {
    fsm: Unqueued<Installer>,
}

impl Install {
    pub fn fsm(&self) -> &Unqueued<Installer> {
        &self.fsm
    }

    /// finny's `dispatch` of a typed event (any level's), then the effects:
    /// `Installer`'s first (`StayAwake` before the first step).
    pub fn dispatch(&mut self, event: impl Into<InstallerEvents>, out: &mut impl Sink<Effect>) -> FsmResult<()> {
        let r = self.fsm.dispatch(event);
        self.fsm.context.out.drain(out);
        let preparing: &mut Preparing = self.fsm.states.as_mut();
        preparing.context.out.drain(out);
        let executing: &mut Executing = self.fsm.states.as_mut();
        executing.context.out.drain(out);
        r
    }

    /// Offers a stage result to the sub-machine (`sub`), then to `Installer`
    /// when the sub-machine isn't running or has no transition for it.
    fn deliver(&mut self, (sub, top): (InstallerEvents, InstallerEvents), out: &mut impl Sink<Effect>) {
        if self.dispatch(sub, out).is_err() {
            let _ = self.dispatch(top, out);
        }
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        let host = Host { reserve_mib: config.reserve_mib, out: Effects::new(Effect::Complete) };
        let mut fsm = Installer::new_with(host, FsmEventQueueNull::new(), InspectNull::new(), FsmTimersNull)
            .expect("the states are built from defaults");
        fsm.start().expect("Idle has no entry action");
        Self { fsm }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        fn prep<E: Into<PreparingEvents> + Clone + Into<InstallerEvents>>(e: E) -> (InstallerEvents, InstallerEvents) {
            let s: PreparingEvents = e.clone().into();
            (s.into(), e.into())
        }
        fn exec<E: Into<ExecutingEvents> + Clone + Into<InstallerEvents>>(e: E) -> (InstallerEvents, InstallerEvents) {
            let s: ExecutingEvents = e.clone().into();
            (s.into(), e.into())
        }
        // `Err(NoTransition)` at the top: ignored.
        match event {
            Event::Begin => {
                let _ = self.dispatch(Begin, out);
            }
            Event::Confirm { phrase_ok } => {
                let _ = self.dispatch(Confirm { phrase_ok }, out);
            }
            Event::Cancel => {
                let _ = self.dispatch(Cancel, out);
            }
            Event::Checked { root_mib } => self.deliver(prep(Checked { root_mib }), out),
            Event::Configured => self.deliver(prep(Configured), out),
            Event::Evaluated { download_mib, unpacked_mib, budget_mib } => {
                self.deliver(prep(Evaluated { download_mib, unpacked_mib, budget_mib }), out)
            }
            Event::Built { closure_mib } => self.deliver(prep(Built { closure_mib }), out),
            Event::Cached => {
                let _ = self.dispatch(Cached, out);
            }
            Event::StepDone => self.deliver(exec(StepDone), out),
            Event::StageFailed => self.deliver(exec(StageFailed), out),
        }
    }

    fn state(&self) -> StateName {
        use FsmCurrentState::State;
        match self.fsm.get_current_states()[0] {
            State(InstallerCurrentState::Idle) | FsmCurrentState::Stopped => StateName::Idle,
            State(InstallerCurrentState::Preparing) => {
                match self.fsm.get_state::<Preparing>().get_current_states()[0] {
                    State(PreparingCurrentState::Checks) | FsmCurrentState::Stopped => StateName::Checks,
                    State(PreparingCurrentState::Configuration) => StateName::Configuration,
                    State(PreparingCurrentState::Evaluation) => StateName::Evaluation,
                    State(PreparingCurrentState::Build) => StateName::Build,
                    State(PreparingCurrentState::Cache) => StateName::Cache,
                }
            }
            State(InstallerCurrentState::Prepared) => StateName::Prepared,
            State(InstallerCurrentState::Cancelling) => StateName::Cancelling,
            State(InstallerCurrentState::Executing) => {
                match self.fsm.get_state::<Executing>().get_current_states()[0] {
                    State(ExecutingCurrentState::Erase) | FsmCurrentState::Stopped => StateName::Erase,
                    State(ExecutingCurrentState::Format) => StateName::Format,
                    State(ExecutingCurrentState::Files) => StateName::Files,
                    State(ExecutingCurrentState::Copy) => StateName::Copy,
                    State(ExecutingCurrentState::BuildOnTarget) => StateName::BuildOnTarget,
                    State(ExecutingCurrentState::Bootloader) => StateName::Bootloader,
                    State(ExecutingCurrentState::Record) => StateName::Record,
                    State(ExecutingCurrentState::Flush) => StateName::Flush,
                }
            }
            State(InstallerCurrentState::Complete) => StateName::Complete,
            State(InstallerCurrentState::Failed) => StateName::Failed,
            State(InstallerCurrentState::Refused) => StateName::Refused,
            State(InstallerCurrentState::Cancelled) => StateName::Cancelled,
        }
    }
}
