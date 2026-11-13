//! `install` with stateforward-sml 2.0.0: `Preparing` and `Executing` are
//! child tables of the session table. Each stage's entry action runs it;
//! entering `Executing` holds the sleep inhibitor and leaving it releases
//! it; `Cancelled` and `Complete` report themselves on entry. A child
//! can't name its parent's states, so preparation ends by processing an
//! internal event the parent takes (`Ready(deferred)` or `Stop`), and
//! execution ends at the child's `X`. The route (`deferred`) that `Erase`
//! .. `Files` carry in the spec is the context's, since a child's initial
//! state can't take a payload from the transition that enters it.

use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

pub const IMPLEMENTED: bool = true;

pub type Out<'a> = &'a mut dyn Sink<Effect>;

#[derive(Clone, Copy)]
pub struct Evaluated {
    pub download_mib: u64,
    pub unpacked_mib: u64,
    pub budget_mib: u64,
}

/// Internal events that end preparation (the route, or a refusal). The
/// fields are private: outside code can't build them, so only the table's
/// `process(...)` sends them.
pub struct Ready(bool);
pub struct Stop(());

sml::sml! {
    Preparing[temporary_context: &mut dyn Sink<Effect>] {
        *Checks + on_entry<_> / run_checks,
         Checks + Checked(u64) / configure = Configuration(u64),
         Configuration(u64) + on_entry<_> / run_configuration,
         Configuration(u64) + Configured / evaluate = Evaluation(u64),
         Evaluation(u64) + on_entry<_> / run_evaluation,
         Evaluation(u64) + Evaluated(Evaluated) [deferred_without_room] / (refuse_deferred, process(Stop(()))),
         Evaluation(u64) + Evaluated(Evaluated) [deferred] / (prepared_deferred, process(Ready(true))),
         Evaluation(u64) + Evaluated(Evaluated) / build = Build(u64),
         Build(u64) + on_entry<_> / run_build,
         Build(u64) + Built(u64) [without_room] / (refuse_built, process(Stop(()))),
         Build(u64) + Built(u64) / cache = Cache(u64),
         Cache(u64) + on_entry<_> / run_cache,
         Cache(u64) + Cached / (prepared_built, process(Ready(false))),
    }

    Executing[temporary_context: &mut dyn Sink<Effect>] {
        *Erase + on_entry<_> / run_erase,
         Erase + StepDone = Format,
         Format + on_entry<_> / run_format,
         Format + StepDone = Files,
         Files + on_entry<_> / run_files,
         Files + StepDone [builds_on_target] = BuildOnTarget,
         Files + StepDone = Copy,
         Copy + on_entry<_> / run_copy,
         BuildOnTarget + on_entry<_> / run_build_on_target,
         Copy | BuildOnTarget + StepDone = Bootloader,
         Bootloader + on_entry<_> / run_bootloader,
         Bootloader + StepDone = Record,
         Record + on_entry<_> / run_record,
         Record + StepDone = Flush,
         Record + StageFailed / warn_no_record = Flush,
         Flush + on_entry<_> / run_flush,
         Flush + StepDone = X,
    }

    Install[temporary_context: &mut dyn Sink<Effect>] {
        *Idle + Begin = state<Preparing>,
         Idle + Cancel = Cancelled,
         Idle + Confirm(bool) / fail_unconfirmed = Failed,
         state<Preparing> + Cancel / interrupt = Cancelling,
         state<Preparing> + Confirm(bool) / interrupt_and_fail = Failed,
         state<Preparing> + StageFailed / fail = Failed,
         state<Preparing> + event<Ready> / prepare = Prepared(bool),
         state<Preparing> + event<Stop> = Refused,
         Prepared(bool) + Cancel = Cancelled,
         Prepared(bool) + Confirm(bool) [phrase_ok] / choose_route = state<Executing>,
         Prepared(bool) + Confirm(bool) / wrong_phrase = Failed,
         Cancelling + Checked(u64) = Cancelled,
         Cancelling + Configured = Cancelled,
         Cancelling + Evaluated(Evaluated) = Cancelled,
         Cancelling + Built(u64) = Cancelled,
         Cancelling + Cached = Cancelled,
         Cancelling + StepDone = Cancelled,
         Cancelling + StageFailed = Cancelled,
         state<Executing> + on_entry<_> / stay_awake,
         state<Executing> + on_exit<_> / allow_sleep,
         state<Executing> + StageFailed / fail_erased = Failed,
         state<Executing> = Complete,
         Complete + on_entry<_> / report_complete,
         Cancelled + on_entry<_> / report_cancelled,
    }
}

/// The context: the reserve, and the route execution takes.
pub struct Installer {
    reserve_mib: u64,
    deferred: bool,
}

type R = Result<(), ()>;
type G = Result<bool, ()>;

fn run(out: Out<'_>, stage: Stage) -> R {
    out.push(Effect::Run(stage));
    Ok(())
}

fn step(out: Out<'_>, n: u8, stage: Stage) -> R {
    out.push(Effect::Progress(n));
    run(out, stage)
}

fn failed(out: Out<'_>, erased: bool) -> R {
    out.push(Effect::Failed { erased });
    Ok(())
}

impl InstallStateMachineContext for Installer {
    fn run_checks(&mut self, out: Out<'_>) -> R {
        run(out, Stage::Checks)
    }
    fn configure(&mut self, _: Out<'_>, root: &u64) -> Result<u64, ()> {
        Ok(*root)
    }
    fn run_configuration(&mut self, out: Out<'_>, _: &u64) -> R {
        run(out, Stage::Configuration)
    }
    fn evaluate(&mut self, _: Out<'_>, root: &u64) -> Result<u64, ()> {
        Ok(*root)
    }
    fn run_evaluation(&mut self, out: Out<'_>, _: &u64) -> R {
        run(out, Stage::Evaluation)
    }
    fn deferred(&self, _: Out<'_>, _: &u64, e: &Evaluated) -> G {
        Ok(e.download_mib > e.budget_mib)
    }
    fn deferred_without_room(&self, _: Out<'_>, root: &u64, e: &Evaluated) -> G {
        Ok(e.download_mib > e.budget_mib && *root < e.unpacked_mib.saturating_add(self.reserve_mib))
    }
    fn refuse_deferred(&mut self, out: Out<'_>, root: &u64, e: &Evaluated) -> R {
        out.push(Effect::Refused { needed_mib: e.unpacked_mib.saturating_add(self.reserve_mib), root_mib: *root });
        Ok(())
    }
    fn prepared_deferred(&mut self, out: Out<'_>, _: &u64, _: &Evaluated) -> R {
        out.push(Effect::Prepared { deferred: true, closure_mib: 0 });
        Ok(())
    }
    fn build(&mut self, _: Out<'_>, root: &u64, _: &Evaluated) -> Result<u64, ()> {
        Ok(*root)
    }
    fn run_build(&mut self, out: Out<'_>, _: &u64) -> R {
        run(out, Stage::Build)
    }
    fn without_room(&self, _: Out<'_>, root: &u64, closure: &u64) -> G {
        Ok(*root < closure.saturating_add(self.reserve_mib))
    }
    fn refuse_built(&mut self, out: Out<'_>, root: &u64, closure: &u64) -> R {
        out.push(Effect::Refused { needed_mib: closure.saturating_add(self.reserve_mib), root_mib: *root });
        Ok(())
    }
    fn cache(&mut self, _: Out<'_>, _: &u64, closure: &u64) -> Result<u64, ()> {
        Ok(*closure)
    }
    fn run_cache(&mut self, out: Out<'_>, _: &u64) -> R {
        run(out, Stage::Cache)
    }
    fn prepared_built(&mut self, out: Out<'_>, closure: &u64) -> R {
        out.push(Effect::Prepared { deferred: false, closure_mib: *closure });
        Ok(())
    }

    fn run_erase(&mut self, out: Out<'_>) -> R {
        step(out, 1, Stage::Erase)
    }
    fn run_format(&mut self, out: Out<'_>) -> R {
        step(out, 2, Stage::Format)
    }
    fn run_files(&mut self, out: Out<'_>) -> R {
        step(out, 3, Stage::Files)
    }
    fn builds_on_target(&self, _: Out<'_>) -> G {
        Ok(self.deferred)
    }
    fn run_copy(&mut self, out: Out<'_>) -> R {
        step(out, 4, Stage::Copy)
    }
    fn run_build_on_target(&mut self, out: Out<'_>) -> R {
        step(out, 4, Stage::BuildOnTarget)
    }
    fn run_bootloader(&mut self, out: Out<'_>) -> R {
        step(out, 5, Stage::Bootloader)
    }
    fn run_record(&mut self, out: Out<'_>) -> R {
        run(out, Stage::Record)
    }
    fn warn_no_record(&mut self, out: Out<'_>) -> R {
        out.push(Effect::Warn(Warning::NoRecord));
        Ok(())
    }
    fn run_flush(&mut self, out: Out<'_>) -> R {
        step(out, 6, Stage::Flush)
    }

    fn fail_unconfirmed(&mut self, out: Out<'_>, _: &bool) -> R {
        failed(out, false)
    }
    fn interrupt(&mut self, out: Out<'_>) -> R {
        out.push(Effect::Interrupt);
        Ok(())
    }
    fn interrupt_and_fail(&mut self, out: Out<'_>, _: &bool) -> R {
        out.push(Effect::Interrupt);
        failed(out, false)
    }
    fn fail(&mut self, out: Out<'_>) -> R {
        failed(out, false)
    }
    fn prepare(&mut self, _: Out<'_>, e: &Ready) -> Result<bool, ()> {
        Ok(e.0)
    }
    fn phrase_ok(&self, _: Out<'_>, _: &bool, phrase_ok: &bool) -> G {
        Ok(*phrase_ok)
    }
    fn choose_route(&mut self, _: Out<'_>, deferred: &bool, _: &bool) -> R {
        self.deferred = *deferred;
        Ok(())
    }
    fn wrong_phrase(&mut self, out: Out<'_>, _: &bool, _: &bool) -> R {
        failed(out, false)
    }
    fn stay_awake(&mut self, out: Out<'_>) -> R {
        out.push(Effect::StayAwake);
        Ok(())
    }
    fn allow_sleep(&mut self, out: Out<'_>) -> R {
        out.push(Effect::AllowSleep);
        Ok(())
    }
    fn fail_erased(&mut self, out: Out<'_>) -> R {
        failed(out, true)
    }
    fn report_complete(&mut self, out: Out<'_>) -> R {
        out.push(Effect::Complete);
        Ok(())
    }
    fn report_cancelled(&mut self, out: Out<'_>) -> R {
        out.push(Effect::Cancelled);
        Ok(())
    }
}

pub struct Install {
    /// The generated machine: its `process_event`, `is` and
    /// `visit_current_states` are the typed API.
    pub machine: InstallStateMachine<Installer>,
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Self { machine: InstallStateMachine::new(Installer { reserve_mib: config.reserve_mib, deferred: false }) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use InstallEvents as E;
        let event = match event {
            Event::Begin => E::Begin,
            Event::Checked { root_mib } => E::Checked(root_mib),
            Event::Configured => E::Configured,
            Event::Evaluated { download_mib, unpacked_mib, budget_mib } => {
                E::Evaluated(Evaluated { download_mib, unpacked_mib, budget_mib })
            }
            Event::Built { closure_mib } => E::Built(closure_mib),
            Event::Cached => E::Cached,
            Event::StepDone => E::StepDone,
            Event::StageFailed => E::StageFailed,
            Event::Confirm { phrase_ok } => E::Confirm(phrase_ok),
            Event::Cancel => E::Cancel,
        };
        // An event no row takes is `Err(InvalidEvent)`: ignored, as the spec says.
        let _ = self.machine.process_event(out, event);
    }

    fn state(&self) -> StateName {
        use InstallExecutingStates as X;
        use InstallPreparingStates as P;
        let m = &self.machine;
        let mut name = StateName::Idle;
        m.visit_current_states(|s| {
            name = match s {
                InstallStates::Idle => StateName::Idle,
                InstallStates::Prepared(_) => StateName::Prepared,
                InstallStates::Cancelling => StateName::Cancelling,
                InstallStates::Complete => StateName::Complete,
                InstallStates::Failed => StateName::Failed,
                InstallStates::Refused => StateName::Refused,
                InstallStates::Cancelled => StateName::Cancelled,
                InstallStates::Preparing => {
                    if m.is(&P::Checks) {
                        StateName::Checks
                    } else if m.is(&P::Configuration(0)) {
                        StateName::Configuration
                    } else if m.is(&P::Evaluation(0)) {
                        StateName::Evaluation
                    } else if m.is(&P::Build(0)) {
                        StateName::Build
                    } else {
                        StateName::Cache
                    }
                }
                InstallStates::Executing => [
                    (X::Erase, StateName::Erase),
                    (X::Format, StateName::Format),
                    (X::Files, StateName::Files),
                    (X::Copy, StateName::Copy),
                    (X::BuildOnTarget, StateName::BuildOnTarget),
                    (X::Bootloader, StateName::Bootloader),
                    (X::Record, StateName::Record),
                ]
                .into_iter()
                .find(|(s, _)| m.is(s))
                .map_or(StateName::Flush, |(_, n)| n),
            }
        });
        name
    }
}
