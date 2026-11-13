//! `install` with fsmentry 0.4.0: the `Session` graph (`Idle`, `Preparing`,
//! `Prepared`, `Cancelling`, `Executing` and the four ends) nests the
//! preparation stages (`Prep`) in `Preparing` and the destructive steps
//! (`Exec`) in `Executing`, so the superstates' `Cancel`, `Confirm` and
//! `StageFailed` rows are one edge each. The nested machines are sealed
//! (`Preparation` starts at `Checks`; `Steps` at `Erase`, and only from a
//! confirmation), and `Prepared` holds a `Route` that only the room checks
//! build. A stage that ends its superstate (`Evaluation`'s deferred route,
//! `Build`'s refusal, `Cache`, `Flush`) is decided on the outer handle,
//! reading the inner state first.

use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

pub const IMPLEMENTED: bool = true;

pub mod prep {
    fsmentry::fsmentry! {
        /// The preparation stages; `Configuration` to `Build` carry the root's size,
        /// `Cache` the built closure's.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Prep {
            Checks -checked-> Configuration(u64) -configured-> Evaluation(u64) -evaluated-> Build(u64) -built-> Cache(u64),
        }
    }
}

pub mod exec {
    fsmentry::fsmentry! {
        /// The destructive steps (each edge: the step done, the next one run); the
        /// first three carry the route (`true`: deferred).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Exec {
            Erase(bool) -> Format(bool) -> Files(bool) -> Copy -> Bootloader -> Record -> Flush,
            Files -> BuildOnTarget -> Bootloader,
        }
    }
}

/// `Preparing`'s data: the preparation stages, built only at `Checks`.
#[derive(Debug, PartialEq, Eq)]
pub struct Preparation(Prep);

/// `Executing`'s data: the destructive steps, built only at `Erase` by a confirmation.
#[derive(Debug, PartialEq, Eq)]
pub struct Steps(Exec);

/// `Prepared`'s data: the route, built only by a passed room check.
#[derive(Debug, PartialEq, Eq)]
pub struct Route {
    deferred: bool,
}

impl Preparation {
    pub fn stage(&self) -> &Prep {
        &self.0
    }

    /// The `Prep` graph's transition API (legal edges only, no effects).
    pub fn entry(&mut self) -> PrepEntry<'_> {
        self.0.entry()
    }
}

impl Steps {
    /// The confirmation: the typed phrase matched (`phrase_ok`) for this route.
    pub fn confirmed(route: &Route, phrase_ok: bool) -> Option<Steps> {
        phrase_ok.then_some(Steps(Exec::Erase(route.deferred)))
    }

    pub fn step(&self) -> &Exec {
        &self.0
    }

    /// The `Exec` graph's transition API (legal edges only, no effects).
    pub fn entry(&mut self) -> ExecEntry<'_> {
        self.0.entry()
    }
}

impl Route {
    pub fn deferred(&self) -> bool {
        self.deferred
    }

    /// The deferred route's room check (GEN-07): the unpacked downloads and
    /// the reserve must fit the root; else the refusal.
    fn deferred_fits(root_mib: u64, unpacked_mib: u64, reserve: u64) -> Result<Route, Effect> {
        let needed_mib = unpacked_mib.saturating_add(reserve);
        if root_mib < needed_mib { Err(Effect::Refused { needed_mib, root_mib }) } else { Ok(Route { deferred: true }) }
    }

    /// The in-memory route, once `Build`'s room check passed and the cache is warm.
    fn in_memory() -> Route {
        Route { deferred: false }
    }
}

pub mod session {
    use super::*;
    fsmentry::fsmentry! {
        #[derive(Debug, PartialEq, Eq)]
        pub enum Session {
            Idle -begin-> Preparing(Preparation) -prepared-> Prepared(Route) -confirm-> Executing(Steps) -complete-> Complete,
            Preparing -refused-> Refused,
            Preparing -cancel-> Cancelling -cancelled-> Cancelled,
            Idle & Prepared -cancel-> Cancelled,
            Idle & Preparing & Prepared & Executing -fail-> Failed,
        }
    }
}

pub use exec::{Exec, ExecEntry};
pub use prep::{Prep, PrepEntry};
pub use session::{Session, SessionEntry};

fn is_result(e: Event) -> bool {
    matches!(
        e,
        Event::Checked { .. }
            | Event::Configured
            | Event::Evaluated { .. }
            | Event::Built { .. }
            | Event::Cached
            | Event::StepDone
            | Event::StageFailed
    )
}

fn step(n: u8, stage: Stage, out: &mut impl Sink<Effect>) {
    out.push(Effect::Progress(n));
    out.push(Effect::Run(stage));
}

pub struct Install {
    reserve: u64,
    session: Session,
}

impl Install {
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// fsmentry's transition API on the live machine: only the current
    /// state's legal edges (and none of `handle`'s effects).
    pub fn entry(&mut self) -> SessionEntry<'_> {
        self.session.entry()
    }
}

/// A preparation stage's result that stays in `Preparing`.
fn prepare(prep: &mut Preparation, event: Event, out: &mut impl Sink<Effect>) {
    match (prep.entry(), event) {
        (PrepEntry::Checks(p), Event::Checked { root_mib }) => {
            out.push(Effect::Run(Stage::Configuration));
            p.checked(root_mib);
        }
        (PrepEntry::Configuration(p), Event::Configured) => {
            out.push(Effect::Run(Stage::Evaluation));
            let root = *p.as_ref();
            p.configured(root);
        }
        (PrepEntry::Evaluation(p), Event::Evaluated { .. }) => {
            out.push(Effect::Run(Stage::Build));
            let root = *p.as_ref();
            p.evaluated(root);
        }
        (PrepEntry::Build(p), Event::Built { closure_mib }) => {
            out.push(Effect::Run(Stage::Cache));
            p.built(closure_mib);
        }
        _ => {}
    }
}

/// A destructive step's result that stays in `Executing`.
fn execute(exec: &mut Exec, event: Event, out: &mut impl Sink<Effect>) {
    match (exec.entry(), event) {
        (ExecEntry::Erase(e), Event::StepDone) => {
            step(2, Stage::Format, out);
            let deferred = *e.as_ref();
            e.format(deferred);
        }
        (ExecEntry::Format(e), Event::StepDone) => {
            step(3, Stage::Files, out);
            let deferred = *e.as_ref();
            e.files(deferred);
        }
        (ExecEntry::Files(e), Event::StepDone) => {
            if *e.as_ref() {
                step(4, Stage::BuildOnTarget, out);
                e.build_on_target();
            } else {
                step(4, Stage::Copy, out);
                e.copy();
            }
        }
        (ExecEntry::Copy(e), Event::StepDone) => {
            step(5, Stage::Bootloader, out);
            e.bootloader();
        }
        (ExecEntry::BuildOnTarget(e), Event::StepDone) => {
            step(5, Stage::Bootloader, out);
            e.bootloader();
        }
        (ExecEntry::Bootloader(e), Event::StepDone) => {
            out.push(Effect::Run(Stage::Record));
            e.record();
        }
        (ExecEntry::Record(e), Event::StepDone | Event::StageFailed) => {
            if event == Event::StageFailed {
                out.push(Effect::Warn(Warning::NoRecord));
            }
            step(6, Stage::Flush, out);
            e.flush();
        }
        _ => {}
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Self { reserve: config.reserve_mib, session: Session::Idle }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let reserve = self.reserve;
        match (self.session.entry(), event) {
            (SessionEntry::Idle(s), Event::Begin) => {
                out.push(Effect::Run(Stage::Checks));
                s.begin(Preparation(Prep::Checks));
            }
            (SessionEntry::Idle(s), Event::Cancel) => {
                out.push(Effect::Cancelled);
                s.cancel();
            }
            (SessionEntry::Idle(s), Event::Confirm { .. }) => {
                out.push(Effect::Failed { erased: false });
                s.fail();
            }
            (SessionEntry::Preparing(s), Event::Cancel) => {
                out.push(Effect::Interrupt);
                s.cancel();
            }
            (SessionEntry::Preparing(s), Event::Confirm { .. }) => {
                out.push(Effect::Interrupt);
                out.push(Effect::Failed { erased: false });
                s.fail();
            }
            (SessionEntry::Preparing(s), Event::StageFailed) => {
                out.push(Effect::Failed { erased: false });
                s.fail();
            }
            (SessionEntry::Preparing(mut s), event) => match (s.as_ref().0, event) {
                (Prep::Evaluation(root), Event::Evaluated { download_mib, unpacked_mib, budget_mib })
                    if download_mib > budget_mib =>
                {
                    match Route::deferred_fits(root, unpacked_mib, reserve) {
                        Ok(route) => {
                            out.push(Effect::Prepared { deferred: true, closure_mib: 0 });
                            s.prepared(route);
                        }
                        Err(refused) => {
                            out.push(refused);
                            s.refused();
                        }
                    }
                }
                (Prep::Build(root), Event::Built { closure_mib }) if root < closure_mib.saturating_add(reserve) => {
                    out.push(Effect::Refused { needed_mib: closure_mib.saturating_add(reserve), root_mib: root });
                    s.refused();
                }
                (Prep::Cache(closure_mib), Event::Cached) => {
                    out.push(Effect::Prepared { deferred: false, closure_mib });
                    s.prepared(Route::in_memory());
                }
                _ => prepare(s.as_mut(), event, out),
            },
            (SessionEntry::Prepared(s), Event::Cancel) => {
                out.push(Effect::Cancelled);
                s.cancel();
            }
            (SessionEntry::Prepared(s), Event::Confirm { phrase_ok }) => {
                match Steps::confirmed(s.as_ref(), phrase_ok) {
                    Some(steps) => {
                        out.push(Effect::StayAwake);
                        step(1, Stage::Erase, out);
                        s.confirm(steps);
                    }
                    None => {
                        out.push(Effect::Failed { erased: false });
                        s.fail();
                    }
                }
            }
            (SessionEntry::Cancelling(s), event) if is_result(event) => {
                out.push(Effect::Cancelled);
                s.cancelled();
            }
            (SessionEntry::Executing(s), Event::StepDone) if s.as_ref().0 == Exec::Flush => {
                out.push(Effect::AllowSleep);
                out.push(Effect::Complete);
                s.complete();
            }
            (SessionEntry::Executing(s), Event::StageFailed) if s.as_ref().0 != Exec::Record => {
                out.push(Effect::AllowSleep);
                out.push(Effect::Failed { erased: true });
                s.fail();
            }
            (SessionEntry::Executing(mut s), event) => execute(&mut s.as_mut().0, event, out),
            _ => {}
        }
    }

    fn state(&self) -> StateName {
        match &self.session {
            Session::Idle => StateName::Idle,
            Session::Preparing(p) => match p.0 {
                Prep::Checks => StateName::Checks,
                Prep::Configuration(_) => StateName::Configuration,
                Prep::Evaluation(_) => StateName::Evaluation,
                Prep::Build(_) => StateName::Build,
                Prep::Cache(_) => StateName::Cache,
            },
            Session::Prepared(_) => StateName::Prepared,
            Session::Cancelling => StateName::Cancelling,
            Session::Executing(e) => match e.0 {
                Exec::Erase(_) => StateName::Erase,
                Exec::Format(_) => StateName::Format,
                Exec::Files(_) => StateName::Files,
                Exec::Copy => StateName::Copy,
                Exec::BuildOnTarget => StateName::BuildOnTarget,
                Exec::Bootloader => StateName::Bootloader,
                Exec::Record => StateName::Record,
                Exec::Flush => StateName::Flush,
            },
            Session::Complete => StateName::Complete,
            Session::Failed => StateName::Failed,
            Session::Refused => StateName::Refused,
            Session::Cancelled => StateName::Cancelled,
        }
    }
}
