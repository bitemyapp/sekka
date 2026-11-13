//! `install` with smlang 0.8.0: one state per stage, carrying what later
//! stages need. Each stage's entry runs it, so `Run(stage)` (after
//! `Progress(n)`, and `StayAwake` at `Erase`) is emitted from smlang's
//! generated `on_entry_*` hooks. Those hooks take only `&mut self` (no
//! temporary context), so this machine collects every effect in a buffer in
//! its context (allocated once) and `handle` drains it into `out`; actions
//! write to the same buffer so the order (exit, action, entry) holds.
//! `Preparing` and `Executing` are emulated with `A | B` patterns, one row
//! per state data type.

use smlang::statemachine;
use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

pub const IMPLEMENTED: bool = true;

/// The root filesystem's room (MiB), from the checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root(pub u64);

/// The built closure's size (MiB), on the in-memory route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closure(pub u64);

/// Which step 4 runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub deferred: bool,
}

/// `Evaluated`: download, unpacked, budget (MiB).
pub type Ev = (u64, u64, u64);

/// The generated machine, kept private: its `new_with_state` and
/// `context_mut` would let callers forge a state.
mod fsm {
    use super::*;

    statemachine! {
        derive_states: [Clone, Copy, Debug],
        derive_events: [Clone, Copy, Debug],
        transitions: {
            *Idle + Begin = Checks,
            Idle | Prepared(Route) + Cancel = Cancelled,
            Idle + Confirm(bool) / unprepared = Failed,
            // Preparing.
            // Action-less, so one row spans the data types; `Interrupt` is
            // `Cancelling`'s entry.
            Checks | Configuration(Root) | Evaluation(Root) | Build(Root) | Cache(Closure) + Cancel = Cancelling,
            Checks + Confirm(bool) / interrupt_confirm = Failed,
            Configuration(Root) | Evaluation(Root) | Build(Root) + Confirm(bool) / interrupt_confirm_root = Failed,
            Cache(Closure) + Confirm(bool) / interrupt_confirm_cache = Failed,
            Checks + StageFailed / unerased = Failed,
            Configuration(Root) | Evaluation(Root) | Build(Root) + StageFailed / unerased_root = Failed,
            Cache(Closure) + StageFailed / unerased_cache = Failed,
            Checks + Checked(u64) / root = Configuration(Root),
            Configuration(Root) + Configured / keep_root = Evaluation(Root),
            Evaluation(Root) + Evaluated(Ev) [within_budget] / build = Build(Root),
            Evaluation(Root) + Evaluated(Ev) [room_unpacked] / prepared_deferred = Prepared(Route),
            Evaluation(Root) + Evaluated(Ev) / refuse_unpacked = Refused,
            Build(Root) + Built(u64) [room_closure] / closure = Cache(Closure),
            Build(Root) + Built(u64) / refuse_closure = Refused,
            Cache(Closure) + Cached / prepared_built = Prepared(Route),
            Cancelling + Checked(u64) = Cancelled,
            Cancelling + Configured = Cancelled,
            Cancelling + Evaluated(Ev) = Cancelled,
            Cancelling + Built(u64) = Cancelled,
            Cancelling + Cached = Cancelled,
            Cancelling + StepDone = Cancelled,
            Cancelling + StageFailed = Cancelled,
            // The irreversible point.
            Prepared(Route) + Confirm(bool) [phrase_ok] / confirmed = Erase(Route),
            Prepared(Route) + Confirm(bool) / wrong_phrase = Failed,
            // Executing.
            Erase(Route) + StepDone / keep_route = Format(Route),
            Format(Route) + StepDone / keep_route = Files(Route),
            Files(Route) + StepDone [deferred] = BuildOnTarget,
            Files(Route) + StepDone = Copy,
            Copy | BuildOnTarget + StepDone = Bootloader,
            Bootloader + StepDone = Record,
            Record + StepDone = Flush,
            Record + StageFailed / warn_no_record = Flush,
            Flush + StepDone = Complete,
            Erase(Route) | Format(Route) | Files(Route) + StageFailed / erased_route = Failed,
            Copy | BuildOnTarget | Bootloader | Flush + StageFailed / erased = Failed,
        }
    }
}

pub use fsm::{Error, Events, States};
use fsm::{StateMachine, StateMachineContext};

/// The context: the reserve, and the effects of the event being handled.
pub struct Installer {
    reserve: u64,
    buf: Vec<Effect>,
}

impl Installer {
    fn new(config: Config) -> Self {
        Self { reserve: config.reserve_mib, buf: Vec::with_capacity(4) }
    }

    /// Effects not yet drained (`Install::handle` drains them).
    pub fn pending(&self) -> &[Effect] {
        &self.buf
    }

    fn push(&mut self, e: Effect) {
        self.buf.push(e);
    }

    fn run(&mut self, stage: Stage) {
        self.push(Effect::Run(stage));
    }

    fn step(&mut self, n: u8, stage: Stage) {
        self.push(Effect::Progress(n));
        self.run(stage);
    }

    fn fail(&mut self, erased: bool) -> R {
        self.push(Effect::Failed { erased });
        Ok(())
    }

    fn interrupt_fail(&mut self) -> R {
        self.push(Effect::Interrupt);
        self.fail(false)
    }

    fn refuse(&mut self, needed_mib: u64, root: &Root) -> R {
        self.push(Effect::Refused { needed_mib, root_mib: root.0 });
        Ok(())
    }

    fn prepared(&mut self, deferred: bool, closure_mib: u64) -> R<Route> {
        self.push(Effect::Prepared { deferred, closure_mib });
        Ok(Route { deferred })
    }

    fn erased_fail(&mut self) -> R {
        self.push(Effect::AllowSleep);
        self.fail(true)
    }
}

type R<T = ()> = Result<T, ()>;

impl StateMachineContext for Installer {
    fn within_budget(&self, _: &Root, &(download, _, budget): &Ev) -> R<bool> {
        Ok(download <= budget)
    }
    fn room_unpacked(&self, root: &Root, &(_, unpacked, _): &Ev) -> R<bool> {
        Ok(root.0 >= unpacked.saturating_add(self.reserve))
    }
    fn room_closure(&self, root: &Root, closure: &u64) -> R<bool> {
        Ok(root.0 >= closure.saturating_add(self.reserve))
    }
    fn phrase_ok(&self, _: &Route, ok: &bool) -> R<bool> {
        Ok(*ok)
    }
    fn deferred(&self, r: &Route) -> R<bool> {
        Ok(r.deferred)
    }

    fn unprepared(&mut self, _: bool) -> R {
        self.fail(false)
    }
    fn interrupt_confirm(&mut self, _: bool) -> R {
        self.interrupt_fail()
    }
    fn interrupt_confirm_root(&mut self, _: &Root, _: bool) -> R {
        self.interrupt_fail()
    }
    fn interrupt_confirm_cache(&mut self, _: &Closure, _: bool) -> R {
        self.interrupt_fail()
    }
    fn unerased(&mut self) -> R {
        self.fail(false)
    }
    fn unerased_root(&mut self, _: &Root) -> R {
        self.fail(false)
    }
    fn unerased_cache(&mut self, _: &Closure) -> R {
        self.fail(false)
    }
    fn root(&mut self, root_mib: u64) -> R<Root> {
        Ok(Root(root_mib))
    }
    fn keep_root(&mut self, root: &Root) -> R<Root> {
        Ok(*root)
    }
    fn build(&mut self, root: &Root, _: Ev) -> R<Root> {
        Ok(*root)
    }
    fn prepared_deferred(&mut self, _: &Root, _: Ev) -> R<Route> {
        self.prepared(true, 0)
    }
    fn refuse_unpacked(&mut self, root: &Root, (_, unpacked, _): Ev) -> R {
        self.refuse(unpacked.saturating_add(self.reserve), root)
    }
    fn closure(&mut self, _: &Root, closure_mib: u64) -> R<Closure> {
        Ok(Closure(closure_mib))
    }
    fn refuse_closure(&mut self, root: &Root, closure_mib: u64) -> R {
        self.refuse(closure_mib.saturating_add(self.reserve), root)
    }
    fn prepared_built(&mut self, c: &Closure) -> R<Route> {
        self.prepared(false, c.0)
    }
    fn confirmed(&mut self, r: &Route, _: bool) -> R<Route> {
        Ok(*r)
    }
    fn wrong_phrase(&mut self, _: &Route, _: bool) -> R {
        self.fail(false)
    }
    fn keep_route(&mut self, r: &Route) -> R<Route> {
        Ok(*r)
    }
    fn warn_no_record(&mut self) -> R {
        self.push(Effect::Warn(Warning::NoRecord));
        Ok(())
    }
    fn erased_route(&mut self, _: &Route) -> R {
        self.erased_fail()
    }
    fn erased(&mut self) -> R {
        self.erased_fail()
    }

    // Each stage's entry runs it.
    fn on_entry_checks(&mut self) {
        self.run(Stage::Checks);
    }
    fn on_entry_configuration(&mut self) {
        self.run(Stage::Configuration);
    }
    fn on_entry_evaluation(&mut self) {
        self.run(Stage::Evaluation);
    }
    fn on_entry_build(&mut self) {
        self.run(Stage::Build);
    }
    fn on_entry_cache(&mut self) {
        self.run(Stage::Cache);
    }
    /// Entering `Executing`: hold the sleep inhibitor.
    fn on_entry_erase(&mut self) {
        self.push(Effect::StayAwake);
        self.step(1, Stage::Erase);
    }
    fn on_entry_format(&mut self) {
        self.step(2, Stage::Format);
    }
    fn on_entry_files(&mut self) {
        self.step(3, Stage::Files);
    }
    fn on_entry_copy(&mut self) {
        self.step(4, Stage::Copy);
    }
    fn on_entry_build_on_target(&mut self) {
        self.step(4, Stage::BuildOnTarget);
    }
    fn on_entry_bootloader(&mut self) {
        self.step(5, Stage::Bootloader);
    }
    fn on_entry_record(&mut self) {
        self.run(Stage::Record);
    }
    fn on_entry_flush(&mut self) {
        self.step(6, Stage::Flush);
    }
    /// Leaving `Executing` for good.
    fn on_entry_complete(&mut self) {
        self.push(Effect::AllowSleep);
        self.push(Effect::Complete);
    }
    /// Stop the running preparation stage.
    fn on_entry_cancelling(&mut self) {
        self.push(Effect::Interrupt);
    }
    fn on_entry_cancelled(&mut self) {
        self.push(Effect::Cancelled);
    }
}

pub struct Install {
    sm: StateMachine<Installer>,
}

impl Install {
    /// The generated `process_event`: the typed transition API (effects
    /// stay in `context().pending()`; `handle` drains them).
    pub fn process_event(&mut self, event: Events) -> Result<&States, Error> {
        self.sm.process_event(event)
    }

    /// The generated state.
    pub fn state_enum(&self) -> &States {
        self.sm.state()
    }

    /// The context, read-only.
    pub fn context(&self) -> &Installer {
        self.sm.context()
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Self { sm: StateMachine::new(Installer::new(config)) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let event = match event {
            Event::Begin => Events::Begin,
            Event::Checked { root_mib } => Events::Checked(root_mib),
            Event::Configured => Events::Configured,
            Event::Evaluated { download_mib, unpacked_mib, budget_mib } => {
                Events::Evaluated((download_mib, unpacked_mib, budget_mib))
            }
            Event::Built { closure_mib } => Events::Built(closure_mib),
            Event::Cached => Events::Cached,
            Event::StepDone => Events::StepDone,
            Event::StageFailed => Events::StageFailed,
            Event::Confirm { phrase_ok } => Events::Confirm(phrase_ok),
            Event::Cancel => Events::Cancel,
        };
        // Events a stage doesn't take are `Err(InvalidEvent)`: ignored.
        let _ = self.sm.process_event(event);
        for e in self.sm.context_mut().buf.drain(..) {
            out.push(e);
        }
    }

    fn state(&self) -> StateName {
        match self.sm.state() {
            States::Idle => StateName::Idle,
            States::Checks => StateName::Checks,
            States::Configuration(_) => StateName::Configuration,
            States::Evaluation(_) => StateName::Evaluation,
            States::Build(_) => StateName::Build,
            States::Cache(_) => StateName::Cache,
            States::Prepared(_) => StateName::Prepared,
            States::Cancelling => StateName::Cancelling,
            States::Erase(_) => StateName::Erase,
            States::Format(_) => StateName::Format,
            States::Files(_) => StateName::Files,
            States::Copy => StateName::Copy,
            States::BuildOnTarget => StateName::BuildOnTarget,
            States::Bootloader => StateName::Bootloader,
            States::Record => StateName::Record,
            States::Flush => StateName::Flush,
            States::Complete => StateName::Complete,
            States::Failed => StateName::Failed,
            States::Refused => StateName::Refused,
            States::Cancelled => StateName::Cancelled,
        }
    }
}
