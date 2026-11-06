//! `install` with state-machines 0.34.0: `Preparing` and `Executing` are
//! superstates; entering `Executing` holds the sleep inhibitor and leaving
//! it releases it (lifecycle hooks), and each stage's entry hook runs it
//! (one generic hook, the stage given by the `Step` trait on its state
//! type). The route and the sizes are leaf data, carried along by `data:`
//! factories.

use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};
use state_machines::state_machine;

pub const IMPLEMENTED: bool = true;

pub use machine::{
    Bootloader, Build, BuildOnTarget, Cache, Cancelled, Cancelling, Checks, Complete, Configuration, Copy, Erase,
    Evaluation, Executing, Failed, Files, Flush, Format, Idle, Install, Installer, InstallerBuildBuiltOutcome,
    InstallerEvaluationEvaluatedOutcome, InstallerFilesStepDoneOutcome, InstallerPreparedConfirmOutcome,
    InstallerRecordStageFailedOutcome, Prepared, Preparing, Record, Refused, Step,
};

/// `Evaluated`'s payload.
#[derive(Clone, Copy, Debug)]
pub struct Evaluated {
    pub download_mib: u64,
    pub unpacked_mib: u64,
    pub budget_mib: u64,
}

/// `Cache`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sizes {
    root_mib: u64,
    closure_mib: u64,
}

#[derive(Debug)]
pub struct Ctx {
    config: Config,
    out: Vec<Effect>,
}

impl Ctx {
    pub fn new(config: Config) -> Self {
        Self { config, out: Vec::with_capacity(crate::out::CAPACITY) }
    }
}

/// The generated machines, their callbacks and the facade. The module is
/// private so that the generated `Dynamic*` wrapper (with its
/// `new_init_state`), its event and its state enums aren't exported.
mod machine {
    use super::*;
    use crate::out::drain;

    state_machine! {
        name: Installer,
        context: Ctx,
        dynamic: true,
        initial: Idle,
        states: [
            Idle,
            superstate Preparing {
                state Checks,
                state Configuration(u64),
                state Evaluation(u64),
                state Build(u64),
                state Cache(Sizes),
            },
            // Whether the route is deferred (built on the target).
            Prepared(bool),
            Cancelling,
            superstate Executing {
                state Erase(bool),
                state Format(bool),
                state Files(bool),
                state Copy,
                state BuildOnTarget,
                state Bootloader,
                state Record,
                state Flush,
            },
            Complete,
            Failed,
            Refused,
            Cancelled,
        ],
        lifecycle: {
            Checks { enter: [run] },
            Configuration { enter: [run] },
            Evaluation { enter: [run] },
            Build { enter: [run] },
            Cache { enter: [run] },
            Cancelling { enter: [interrupt] },
            Executing { enter: [stay_awake], exit: [allow_sleep] },
            Erase { enter: [run] },
            Format { enter: [run] },
            Files { enter: [run] },
            Copy { enter: [run] },
            BuildOnTarget { enter: [run] },
            Bootloader { enter: [run] },
            Record { enter: [run] },
            Flush { enter: [run] },
            Complete { enter: [complete] },
            Cancelled { enter: [cancelled] },
        },
        events {
            begin {
                transition: { from: Idle, to: Checks }
            }
            // `Erase` is the irreversible point: no `Cancel` from `Executing`.
            cancel {
                transition: { from: [Idle, Prepared], to: Cancelled }
                transition: { from: Preparing, to: Cancelling }
            }
            confirm {
                payload: bool,
                branching: true,
                transition: { from: Idle, to: Failed, fallback: true, after: [failed_unerased_for] }
                // A protocol error during preparation (GEN-06).
                transition: { from: Preparing, to: Failed, fallback: true, before: [interrupt_for], after: [failed_unerased_for] }
                transition: { from: Prepared, to: Erase, guards: [phrase_ok], data: route_confirmed }
                transition: { from: Prepared, to: Failed, fallback: true, after: [failed_unerased_for] }
            }
            checked {
                payload: u64,
                transition: { from: Checks, to: Configuration, data: root_checked }
                transition: { from: Cancelling, to: Cancelled }
            }
            configured {
                transition: { from: Configuration, to: Evaluation, data: root_kept }
                transition: { from: Cancelling, to: Cancelled }
            }
            evaluated {
                payload: Evaluated,
                branching: true,
                transition: { from: Evaluation, to: Build, guards: [fits_memory], data: root_kept_for }
                // Deferred: the unpacked downloads must fit the root (GEN-07).
                transition: { from: Evaluation, to: Prepared, guards: [fits_unpacked], before: [report_deferred], data: deferred }
                transition: { from: Evaluation, to: Refused, fallback: true, before: [refuse_unpacked] }
                transition: { from: Cancelling, to: Cancelled, fallback: true }
            }
            built {
                payload: u64,
                branching: true,
                transition: { from: Build, to: Cache, guards: [fits_closure], data: sizes }
                transition: { from: Build, to: Refused, fallback: true, before: [refuse_closure] }
                transition: { from: Cancelling, to: Cancelled, fallback: true }
            }
            cached {
                transition: { from: Cache, to: Prepared, before: [report_built], data: in_memory }
                transition: { from: Cancelling, to: Cancelled }
            }
            // `Files` chooses step 4, so the event branches; every other
            // candidate is then a `fallback`.
            step_done {
                branching: true,
                transition: { from: Erase, to: Format, fallback: true, data: route_kept }
                transition: { from: Format, to: Files, fallback: true, data: route_kept }
                transition: { from: Files, to: BuildOnTarget, guards: [is_deferred] }
                transition: { from: Files, to: Copy, fallback: true }
                transition: { from: [Copy, BuildOnTarget], to: Bootloader, fallback: true }
                transition: { from: Bootloader, to: Record, fallback: true }
                transition: { from: Record, to: Flush, fallback: true }
                transition: { from: Flush, to: Complete, fallback: true }
                transition: { from: Cancelling, to: Cancelled, fallback: true }
            }
            stage_failed {
                hierarchical: true,
                transition: { from: Preparing, to: Failed, after: [failed_unerased] }
                transition: { from: Cancelling, to: Cancelled }
                // A missing record is a warning: the system is installed.
                transition: { from: Record, to: Flush, before: [warn_no_record] }
                transition: { from: Executing, to: Failed, after: [failed_erased] }
            }
        }
    }

    /// The stage each stage state's entry runs, and its numbered step.
    pub trait Step {
        const RUN: Stage;
        const PROGRESS: Option<u8> = None;
    }

    impl Step for Checks {
        const RUN: Stage = Stage::Checks;
    }
    impl Step for Configuration {
        const RUN: Stage = Stage::Configuration;
    }
    impl Step for Evaluation {
        const RUN: Stage = Stage::Evaluation;
    }
    impl Step for Build {
        const RUN: Stage = Stage::Build;
    }
    impl Step for Cache {
        const RUN: Stage = Stage::Cache;
    }
    impl Step for Erase {
        const RUN: Stage = Stage::Erase;
        const PROGRESS: Option<u8> = Some(1);
    }
    impl Step for Format {
        const RUN: Stage = Stage::Format;
        const PROGRESS: Option<u8> = Some(2);
    }
    impl Step for Files {
        const RUN: Stage = Stage::Files;
        const PROGRESS: Option<u8> = Some(3);
    }
    impl Step for Copy {
        const RUN: Stage = Stage::Copy;
        const PROGRESS: Option<u8> = Some(4);
    }
    impl Step for BuildOnTarget {
        const RUN: Stage = Stage::BuildOnTarget;
        const PROGRESS: Option<u8> = Some(4);
    }
    impl Step for Bootloader {
        const RUN: Stage = Stage::Bootloader;
        const PROGRESS: Option<u8> = Some(5);
    }
    impl Step for Record {
        const RUN: Stage = Stage::Record;
    }
    impl Step for Flush {
        const RUN: Stage = Stage::Flush;
        const PROGRESS: Option<u8> = Some(6);
    }

    impl<S: Step> Installer<S> {
        /// A stage's entry hook.
        fn run(&mut self) {
            if let Some(n) = S::PROGRESS {
                self.ctx.out.push(Effect::Progress(n));
            }
            self.ctx.out.push(Effect::Run(S::RUN));
        }
    }

    impl<S> Installer<S> {
        fn root(&self) -> u64 {
            let root = self.state_data_configuration().or(self.state_data_evaluation()).or(self.state_data_build());
            root.copied().expect("entered with the root's size")
        }

        fn route(&self) -> bool {
            let route = self.state_data_prepared().or(self.state_data_erase());
            route.or(self.state_data_format()).or(self.state_data_files()).copied().expect("entered with its route")
        }

        // Hooks.

        fn stay_awake(&mut self) {
            self.ctx.out.push(Effect::StayAwake);
        }

        fn allow_sleep(&mut self) {
            self.ctx.out.push(Effect::AllowSleep);
        }

        fn interrupt(&mut self) {
            self.ctx.out.push(Effect::Interrupt);
        }

        fn complete(&mut self) {
            self.ctx.out.push(Effect::Complete);
        }

        fn cancelled(&mut self) {
            self.ctx.out.push(Effect::Cancelled);
        }

        // Guards.

        fn phrase_ok(&self, _: &Ctx, &ok: &bool) -> bool {
            ok
        }

        fn fits_memory(&self, _: &Ctx, e: &Evaluated) -> bool {
            e.download_mib <= e.budget_mib
        }

        fn fits_unpacked(&self, ctx: &Ctx, e: &Evaluated) -> bool {
            self.root() >= e.unpacked_mib.saturating_add(ctx.config.reserve_mib)
        }

        fn fits_closure(&self, ctx: &Ctx, &closure: &u64) -> bool {
            self.root() >= closure.saturating_add(ctx.config.reserve_mib)
        }

        fn is_deferred(&self, _: &Ctx) -> bool {
            self.state_data_files() == Some(&true)
        }

        // Factories (a factory's arity follows its event's payload too).

        fn root_checked(&mut self, &mut root: &mut u64) -> u64 {
            root
        }

        fn root_kept(&mut self) -> u64 {
            self.root()
        }

        fn root_kept_for(&mut self, _: &mut Evaluated) -> u64 {
            self.root()
        }

        fn sizes(&mut self, &mut closure_mib: &mut u64) -> Sizes {
            Sizes { root_mib: self.root(), closure_mib }
        }

        fn deferred(&mut self, _: &mut Evaluated) -> bool {
            true
        }

        fn in_memory(&mut self) -> bool {
            false
        }

        fn route_confirmed(&mut self, _: &mut bool) -> bool {
            self.route()
        }

        fn route_kept(&mut self) -> bool {
            self.route()
        }

        // Callbacks.

        fn interrupt_for(&mut self, _: &bool) {
            self.interrupt();
        }

        fn failed_unerased(&mut self) {
            self.ctx.out.push(Effect::Failed { erased: false });
        }

        fn failed_unerased_for(&mut self, _: &bool) {
            self.failed_unerased();
        }

        fn failed_erased(&mut self) {
            self.ctx.out.push(Effect::Failed { erased: true });
        }

        fn report_deferred(&mut self, _: &Evaluated) {
            self.ctx.out.push(Effect::Prepared { deferred: true, closure_mib: 0 });
        }

        fn report_built(&mut self) {
            let closure_mib = self.state_data_cache().expect("entered with the sizes").closure_mib;
            self.ctx.out.push(Effect::Prepared { deferred: false, closure_mib });
        }

        fn refuse_unpacked(&mut self, e: &Evaluated) {
            let needed_mib = e.unpacked_mib.saturating_add(self.ctx.config.reserve_mib);
            self.ctx.out.push(Effect::Refused { needed_mib, root_mib: self.root() });
        }

        fn refuse_closure(&mut self, &closure: &u64) {
            let needed_mib = closure.saturating_add(self.ctx.config.reserve_mib);
            self.ctx.out.push(Effect::Refused { needed_mib, root_mib: self.root() });
        }

        fn warn_no_record(&mut self) {
            self.ctx.out.push(Effect::Warn(Warning::NoRecord));
        }
    }

    /// The `Machine` facade over the generated runtime wrapper.
    pub struct Install {
        machine: DynamicInstaller,
    }

    /// The context inside the wrapper (`None` once a panicking callback
    /// poisoned it). `inner` and `AnyInstallerState` are private generated
    /// items, visible here because the macro expanded in this module.
    impl DynamicInstaller {
        fn ctx(&mut self) -> Option<&mut Ctx> {
            Some(match self.inner.as_mut()? {
                AnyInstallerState::Idle(m) => &mut m.ctx,
                AnyInstallerState::Checks(m) => &mut m.ctx,
                AnyInstallerState::Configuration(m) => &mut m.ctx,
                AnyInstallerState::Evaluation(m) => &mut m.ctx,
                AnyInstallerState::Build(m) => &mut m.ctx,
                AnyInstallerState::Cache(m) => &mut m.ctx,
                AnyInstallerState::Prepared(m) => &mut m.ctx,
                AnyInstallerState::Cancelling(m) => &mut m.ctx,
                AnyInstallerState::Erase(m) => &mut m.ctx,
                AnyInstallerState::Format(m) => &mut m.ctx,
                AnyInstallerState::Files(m) => &mut m.ctx,
                AnyInstallerState::Copy(m) => &mut m.ctx,
                AnyInstallerState::BuildOnTarget(m) => &mut m.ctx,
                AnyInstallerState::Bootloader(m) => &mut m.ctx,
                AnyInstallerState::Record(m) => &mut m.ctx,
                AnyInstallerState::Flush(m) => &mut m.ctx,
                AnyInstallerState::Complete(m) => &mut m.ctx,
                AnyInstallerState::Failed(m) => &mut m.ctx,
                AnyInstallerState::Refused(m) => &mut m.ctx,
                AnyInstallerState::Cancelled(m) => &mut m.ctx,
            })
        }
    }

    impl<S> Installer<S> {
        /// Takes the effects the callbacks buffered, for driving the typed
        /// machine directly (the compile-fail probes do).
        pub fn take_effects(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.ctx.out)
        }
    }

    impl Machine for Install {
        fn new(config: Config) -> Self {
            Self { machine: DynamicInstaller::new(Ctx::new(config)) }
        }

        fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
            let event = match event {
                Event::Begin => InstallerEvent::Begin,
                Event::Checked { root_mib } => InstallerEvent::Checked(root_mib),
                Event::Configured => InstallerEvent::Configured,
                Event::Evaluated { download_mib, unpacked_mib, budget_mib } => {
                    InstallerEvent::Evaluated(Evaluated { download_mib, unpacked_mib, budget_mib })
                }
                Event::Built { closure_mib } => InstallerEvent::Built(closure_mib),
                Event::Cached => InstallerEvent::Cached,
                Event::StepDone => InstallerEvent::StepDone,
                Event::StageFailed => InstallerEvent::StageFailed,
                Event::Confirm { phrase_ok } => InstallerEvent::Confirm(phrase_ok),
                Event::Cancel => InstallerEvent::Cancel,
            };
            // `Err`: ignored here (terminal states ignore everything).
            let _ = self.machine.handle(event);
            if let Some(ctx) = self.machine.ctx() {
                drain(&mut ctx.out, out);
            }
        }

        fn state(&self) -> StateName {
            match self.machine.current_state() {
                InstallerState::Idle => StateName::Idle,
                InstallerState::Checks => StateName::Checks,
                InstallerState::Configuration => StateName::Configuration,
                InstallerState::Evaluation => StateName::Evaluation,
                InstallerState::Build => StateName::Build,
                InstallerState::Cache => StateName::Cache,
                InstallerState::Prepared => StateName::Prepared,
                InstallerState::Cancelling => StateName::Cancelling,
                InstallerState::Erase => StateName::Erase,
                InstallerState::Format => StateName::Format,
                InstallerState::Files => StateName::Files,
                InstallerState::Copy => StateName::Copy,
                InstallerState::BuildOnTarget => StateName::BuildOnTarget,
                InstallerState::Bootloader => StateName::Bootloader,
                InstallerState::Record => StateName::Record,
                InstallerState::Flush => StateName::Flush,
                InstallerState::Complete => StateName::Complete,
                InstallerState::Failed => StateName::Failed,
                InstallerState::Refused => StateName::Refused,
                InstallerState::Cancelled => StateName::Cancelled,
            }
        }
    }
}
