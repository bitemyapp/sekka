//! `install` with statig 0.4.1: the stages are leaves under the superstates
//! `preparing` (which handles `Cancel`, an early `Confirm` and failures) and
//! `executing` (whose entry and exit actions hold and release the sleep
//! inhibitor, and which handles failures past the irreversible point). Each
//! stage's entry action runs it; each terminal state's entry action reports
//! the end, so the report follows `executing`'s exit.

use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};
use statig::prelude::*;

use crate::{Nowhere, Out};

pub const IMPLEMENTED: bool = true;

/// The shared storage.
pub struct Helper {
    reserve: u64,
}

impl Helper {
    /// The refusal when the root filesystem has no room for `size` MiB plus the reserve.
    fn refusal(&self, size: u64, root: u64) -> Option<State> {
        let needed = size.saturating_add(self.reserve);
        (root < needed).then(|| State::refused(needed, root))
    }
}

fn run(context: &mut Out<'_, Effect>, stage: Stage) {
    context.push(Effect::Run(stage));
}

fn step(context: &mut Out<'_, Effect>, n: u8, stage: Stage) {
    context.push(Effect::Progress(n));
    context.push(Effect::Run(stage));
}

#[state_machine(initial = "State::idle()", state(derive(Debug, Clone, Copy, PartialEq, Eq)))]
impl Helper {
    #[state]
    fn idle(event: &Event) -> Outcome<State> {
        match event {
            Event::Begin => Transition(State::checks()),
            Event::Cancel => Transition(State::cancelled()),
            Event::Confirm { .. } => Transition(State::failed(false)),
            _ => Handled,
        }
    }

    /// Nothing is written yet: `Cancel` interrupts, and a confirmation is a
    /// protocol error.
    #[superstate]
    fn preparing(context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match event {
            Event::Cancel => {
                context.push(Effect::Interrupt);
                Transition(State::cancelling())
            }
            Event::Confirm { .. } => {
                context.push(Effect::Interrupt);
                Transition(State::failed(false))
            }
            Event::StageFailed => Transition(State::failed(false)),
            _ => Handled,
        }
    }

    #[state(superstate = "preparing", entry_action = "run_checks")]
    fn checks(event: &Event) -> Outcome<State> {
        match *event {
            Event::Checked { root_mib } => Transition(State::configuration(root_mib)),
            _ => Super,
        }
    }

    #[action]
    fn run_checks(context: &mut Out<'_, Effect>) {
        run(context, Stage::Checks);
    }

    #[state(superstate = "preparing", entry_action = "run_configuration")]
    fn configuration(root: &mut u64, event: &Event) -> Outcome<State> {
        match event {
            Event::Configured => Transition(State::evaluation(*root)),
            _ => Super,
        }
    }

    #[action]
    fn run_configuration(context: &mut Out<'_, Effect>) {
        run(context, Stage::Configuration);
    }

    #[state(superstate = "preparing", entry_action = "run_evaluation")]
    fn evaluation(&self, root: &mut u64, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            // The downloads don't fit in memory: build on the target, if the
            // unpacked selection fits on its root (GEN-07).
            Event::Evaluated { download_mib, unpacked_mib, budget_mib } if download_mib > budget_mib => {
                if let Some(refused) = self.refusal(unpacked_mib, *root) {
                    return Transition(refused);
                }
                context.push(Effect::Prepared { deferred: true, closure_mib: 0 });
                Transition(State::prepared(true))
            }
            Event::Evaluated { .. } => Transition(State::build(*root)),
            _ => Super,
        }
    }

    #[action]
    fn run_evaluation(context: &mut Out<'_, Effect>) {
        run(context, Stage::Evaluation);
    }

    #[state(superstate = "preparing", entry_action = "run_build")]
    fn build(&self, root: &mut u64, event: &Event) -> Outcome<State> {
        match *event {
            Event::Built { closure_mib } => {
                Transition(self.refusal(closure_mib, *root).unwrap_or(State::cache(closure_mib)))
            }
            _ => Super,
        }
    }

    #[action]
    fn run_build(context: &mut Out<'_, Effect>) {
        run(context, Stage::Build);
    }

    #[state(superstate = "preparing", entry_action = "run_cache")]
    fn cache(closure: &mut u64, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match event {
            Event::Cached => {
                context.push(Effect::Prepared { deferred: false, closure_mib: *closure });
                Transition(State::prepared(false))
            }
            _ => Super,
        }
    }

    #[action]
    fn run_cache(context: &mut Out<'_, Effect>) {
        run(context, Stage::Cache);
    }

    /// Waiting for the typed confirmation.
    #[state]
    fn prepared(deferred: &mut bool, event: &Event) -> Outcome<State> {
        match *event {
            Event::Cancel => Transition(State::cancelled()),
            Event::Confirm { phrase_ok: true } => Transition(State::erase(*deferred)),
            Event::Confirm { phrase_ok: false } => Transition(State::failed(false)),
            _ => Handled,
        }
    }

    /// `Cancel` arrived during a stage: its result, whatever it is, ends the session.
    #[state]
    fn cancelling(event: &Event) -> Outcome<State> {
        match event {
            Event::Checked { .. }
            | Event::Configured
            | Event::Evaluated { .. }
            | Event::Built { .. }
            | Event::Cached
            | Event::StepDone
            | Event::StageFailed => Transition(State::cancelled()),
            _ => Handled,
        }
    }

    /// Past the irreversible point: the disk is being written; `Cancel` is ignored.
    #[superstate(entry_action = "stay_awake", exit_action = "allow_sleep")]
    fn executing(event: &Event) -> Outcome<State> {
        match event {
            Event::StageFailed => Transition(State::failed(true)),
            _ => Handled,
        }
    }

    #[action]
    fn stay_awake(context: &mut Out<'_, Effect>) {
        context.push(Effect::StayAwake);
    }

    #[action]
    fn allow_sleep(context: &mut Out<'_, Effect>) {
        context.push(Effect::AllowSleep);
    }

    #[state(superstate = "executing", entry_action = "run_erase")]
    fn erase(deferred: &mut bool, event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone => Transition(State::format(*deferred)),
            _ => Super,
        }
    }

    #[action]
    fn run_erase(context: &mut Out<'_, Effect>) {
        step(context, 1, Stage::Erase);
    }

    #[state(superstate = "executing", entry_action = "run_format")]
    fn format(deferred: &mut bool, event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone => Transition(State::files(*deferred)),
            _ => Super,
        }
    }

    #[action]
    fn run_format(context: &mut Out<'_, Effect>) {
        step(context, 2, Stage::Format);
    }

    #[state(superstate = "executing", entry_action = "run_files")]
    fn files(deferred: &mut bool, event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone if *deferred => Transition(State::build_on_target()),
            Event::StepDone => Transition(State::copy()),
            _ => Super,
        }
    }

    #[action]
    fn run_files(context: &mut Out<'_, Effect>) {
        step(context, 3, Stage::Files);
    }

    #[state(superstate = "executing", entry_action = "run_copy")]
    fn copy(event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone => Transition(State::bootloader()),
            _ => Super,
        }
    }

    #[action]
    fn run_copy(context: &mut Out<'_, Effect>) {
        step(context, 4, Stage::Copy);
    }

    #[state(superstate = "executing", entry_action = "run_build_on_target")]
    fn build_on_target(event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone => Transition(State::bootloader()),
            _ => Super,
        }
    }

    #[action]
    fn run_build_on_target(context: &mut Out<'_, Effect>) {
        step(context, 4, Stage::BuildOnTarget);
    }

    #[state(superstate = "executing", entry_action = "run_bootloader")]
    fn bootloader(event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone => Transition(State::record()),
            _ => Super,
        }
    }

    #[action]
    fn run_bootloader(context: &mut Out<'_, Effect>) {
        step(context, 5, Stage::Bootloader);
    }

    /// The `install.json` record: a failure is only a warning.
    #[state(superstate = "executing", entry_action = "run_record")]
    fn record(context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone => Transition(State::flush()),
            Event::StageFailed => {
                context.push(Effect::Warn(Warning::NoRecord));
                Transition(State::flush())
            }
            _ => Super,
        }
    }

    #[action]
    fn run_record(context: &mut Out<'_, Effect>) {
        run(context, Stage::Record);
    }

    #[state(superstate = "executing", entry_action = "run_flush")]
    fn flush(event: &Event) -> Outcome<State> {
        match event {
            Event::StepDone => Transition(State::complete()),
            _ => Super,
        }
    }

    #[action]
    fn run_flush(context: &mut Out<'_, Effect>) {
        step(context, 6, Stage::Flush);
    }

    #[state(entry_action = "report_complete")]
    fn complete() -> Outcome<State> {
        Handled
    }

    #[action]
    fn report_complete(context: &mut Out<'_, Effect>) {
        context.push(Effect::Complete);
    }

    #[state(entry_action = "report_failed", local_storage("erased: bool"))]
    fn failed() -> Outcome<State> {
        Handled
    }

    #[action]
    fn report_failed(erased: &mut bool, context: &mut Out<'_, Effect>) {
        context.push(Effect::Failed { erased: *erased });
    }

    #[state(entry_action = "report_refused", local_storage("needed: u64", "root: u64"))]
    fn refused() -> Outcome<State> {
        Handled
    }

    #[action]
    fn report_refused(needed: &mut u64, root: &mut u64, context: &mut Out<'_, Effect>) {
        context.push(Effect::Refused { needed_mib: *needed, root_mib: *root });
    }

    #[state(entry_action = "report_cancelled")]
    fn cancelled() -> Outcome<State> {
        Handled
    }

    #[action]
    fn report_cancelled(context: &mut Out<'_, Effect>) {
        context.push(Effect::Cancelled);
    }
}

pub struct Install {
    machine: statig::blocking::InitializedStateMachine<Helper>,
}

impl Install {
    pub fn state_enum(&self) -> &State {
        self.machine.state()
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        let machine = Helper { reserve: config.reserve_mib }.uninitialized_state_machine();
        Self { machine: machine.init_with_context(&mut Out(&mut Nowhere)) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.machine.handle_with_context(&event, &mut Out(out));
    }

    fn state(&self) -> StateName {
        match self.machine.state() {
            State::Idle {} => StateName::Idle,
            State::Checks {} => StateName::Checks,
            State::Configuration { .. } => StateName::Configuration,
            State::Evaluation { .. } => StateName::Evaluation,
            State::Build { .. } => StateName::Build,
            State::Cache { .. } => StateName::Cache,
            State::Prepared { .. } => StateName::Prepared,
            State::Cancelling {} => StateName::Cancelling,
            State::Erase { .. } => StateName::Erase,
            State::Format { .. } => StateName::Format,
            State::Files { .. } => StateName::Files,
            State::Copy {} => StateName::Copy,
            State::BuildOnTarget {} => StateName::BuildOnTarget,
            State::Bootloader {} => StateName::Bootloader,
            State::Record {} => StateName::Record,
            State::Flush {} => StateName::Flush,
            State::Complete {} => StateName::Complete,
            State::Failed { .. } => StateName::Failed,
            State::Refused { .. } => StateName::Refused,
            State::Cancelled {} => StateName::Cancelled,
        }
    }
}
