//! `install` as hand-written typestate: one type per stage. `Prepared` is
//! only built by the checks that both routes run, the destructive steps
//! start only from `Prepared::confirm`, each step's type has only its
//! successor, and nothing past `Erase` can be cancelled.

use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

pub const IMPLEMENTED: bool = true;

#[derive(Debug)]
pub struct Idle(());
#[derive(Debug)]
pub struct Checks(());
#[derive(Debug)]
pub struct Configuration {
    root: u64,
}
#[derive(Debug)]
pub struct Evaluation {
    root: u64,
}
#[derive(Debug)]
pub struct Build {
    root: u64,
}
#[derive(Debug)]
pub struct Cache {
    closure: u64,
}
#[derive(Debug)]
pub struct Prepared {
    deferred: bool,
}
#[derive(Debug)]
pub struct Cancelling(());
/// Steps 1–3 carry the route to step 4.
#[derive(Debug)]
pub struct Erase {
    deferred: bool,
}
#[derive(Debug)]
pub struct Format {
    deferred: bool,
}
#[derive(Debug)]
pub struct Files {
    deferred: bool,
}
#[derive(Debug)]
pub struct Copy(());
#[derive(Debug)]
pub struct BuildOnTarget(());
#[derive(Debug)]
pub struct Bootloader(());
#[derive(Debug)]
pub struct Record(());
#[derive(Debug)]
pub struct Flush(());
#[derive(Debug)]
pub enum End {
    Complete,
    Failed,
    Refused,
    Cancelled,
}

fn run<T>(stage: Stage, next: T, out: &mut impl Sink<Effect>) -> T {
    out.push(Effect::Run(stage));
    next
}
fn step<T>(n: u8, stage: Stage, next: T, out: &mut impl Sink<Effect>) -> T {
    out.push(Effect::Progress(n));
    run(stage, next, out)
}
fn fail_before_erase(out: &mut impl Sink<Effect>) -> End {
    out.push(Effect::Failed { erased: false });
    End::Failed
}
fn fail_after_erase(out: &mut impl Sink<Effect>) -> End {
    out.push(Effect::AllowSleep);
    out.push(Effect::Failed { erased: true });
    End::Failed
}
fn refuse(needed_mib: u64, root_mib: u64, out: &mut impl Sink<Effect>) -> End {
    out.push(Effect::Refused { needed_mib, root_mib });
    End::Refused
}

/// What preparation stages share: a failure, a cancel, an early confirmation.
macro_rules! preparing {
    ($($t:ident),*) => {$(
        impl $t {
            pub fn failed(self, out: &mut impl Sink<Effect>) -> End {
                fail_before_erase(out)
            }
            pub fn cancel(self, out: &mut impl Sink<Effect>) -> Cancelling {
                out.push(Effect::Interrupt);
                Cancelling(())
            }
            /// A confirmation before the review is a protocol error.
            pub fn confirm_early(self, out: &mut impl Sink<Effect>) -> End {
                out.push(Effect::Interrupt);
                fail_before_erase(out)
            }
        }
    )*};
}
preparing!(Checks, Configuration, Evaluation, Build, Cache);

/// What destructive steps share: a failure.
macro_rules! executing {
    ($($t:ident),*) => {$(
        impl $t {
            pub fn failed(self, out: &mut impl Sink<Effect>) -> End {
                fail_after_erase(out)
            }
        }
    )*};
}
executing!(Erase, Format, Files, Copy, BuildOnTarget, Bootloader, Flush);

impl Idle {
    pub fn new() -> Self {
        Idle(())
    }
    pub fn begin(self, out: &mut impl Sink<Effect>) -> Checks {
        run(Stage::Checks, Checks(()), out)
    }
    pub fn cancel(self, out: &mut impl Sink<Effect>) -> End {
        out.push(Effect::Cancelled);
        End::Cancelled
    }
    pub fn confirm_early(self, out: &mut impl Sink<Effect>) -> End {
        fail_before_erase(out)
    }
}

impl Default for Idle {
    fn default() -> Self {
        Self::new()
    }
}

impl Checks {
    pub fn checked(self, root_mib: u64, out: &mut impl Sink<Effect>) -> Configuration {
        run(Stage::Configuration, Configuration { root: root_mib }, out)
    }
}

impl Configuration {
    pub fn configured(self, out: &mut impl Sink<Effect>) -> Evaluation {
        run(Stage::Evaluation, Evaluation { root: self.root }, out)
    }
}

/// Where the evaluation leads.
pub enum Route {
    Build(Build),
    Deferred(Prepared),
    Refused(End),
}

impl Evaluation {
    pub fn evaluated(
        self,
        c: &Config,
        download_mib: u64,
        unpacked_mib: u64,
        budget_mib: u64,
        out: &mut impl Sink<Effect>,
    ) -> Route {
        if download_mib <= budget_mib {
            return Route::Build(run(Stage::Build, Build { root: self.root }, out));
        }
        let needed = unpacked_mib.saturating_add(c.reserve_mib);
        if self.root < needed {
            return Route::Refused(refuse(needed, self.root, out));
        }
        out.push(Effect::Prepared { deferred: true, closure_mib: 0 });
        Route::Deferred(Prepared { deferred: true })
    }
}

impl Build {
    pub fn built(self, c: &Config, closure_mib: u64, out: &mut impl Sink<Effect>) -> Result<Cache, End> {
        let needed = closure_mib.saturating_add(c.reserve_mib);
        if self.root < needed {
            return Err(refuse(needed, self.root, out));
        }
        Ok(run(Stage::Cache, Cache { closure: closure_mib }, out))
    }
}

impl Cache {
    pub fn cached(self, out: &mut impl Sink<Effect>) -> Prepared {
        out.push(Effect::Prepared { deferred: false, closure_mib: self.closure });
        Prepared { deferred: false }
    }
}

impl Prepared {
    /// The irreversible point: from here on the disk is written.
    pub fn confirm(self, phrase_ok: bool, out: &mut impl Sink<Effect>) -> Result<Erase, End> {
        if !phrase_ok {
            return Err(fail_before_erase(out));
        }
        out.push(Effect::StayAwake);
        Ok(step(1, Stage::Erase, Erase { deferred: self.deferred }, out))
    }
    pub fn cancel(self, out: &mut impl Sink<Effect>) -> End {
        out.push(Effect::Cancelled);
        End::Cancelled
    }
}

impl Cancelling {
    /// The interrupted stage ended, whatever it reported.
    pub fn stopped(self, out: &mut impl Sink<Effect>) -> End {
        out.push(Effect::Cancelled);
        End::Cancelled
    }
}

/// Step 4, by route.
pub enum Step4 {
    Copy(Copy),
    BuildOnTarget(BuildOnTarget),
}

impl Erase {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Format {
        step(2, Stage::Format, Format { deferred: self.deferred }, out)
    }
}
impl Format {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Files {
        step(3, Stage::Files, Files { deferred: self.deferred }, out)
    }
}
impl Files {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Step4 {
        if self.deferred {
            Step4::BuildOnTarget(step(4, Stage::BuildOnTarget, BuildOnTarget(()), out))
        } else {
            Step4::Copy(step(4, Stage::Copy, Copy(()), out))
        }
    }
}
impl Copy {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Bootloader {
        step(5, Stage::Bootloader, Bootloader(()), out)
    }
}
impl BuildOnTarget {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Bootloader {
        step(5, Stage::Bootloader, Bootloader(()), out)
    }
}
impl Bootloader {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Record {
        run(Stage::Record, Record(()), out)
    }
}
impl Record {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Flush {
        step(6, Stage::Flush, Flush(()), out)
    }
    /// The system is installed: a missing record is only a warning.
    pub fn failed(self, out: &mut impl Sink<Effect>) -> Flush {
        out.push(Effect::Warn(Warning::NoRecord));
        step(6, Stage::Flush, Flush(()), out)
    }
}
impl Flush {
    pub fn done(self, out: &mut impl Sink<Effect>) -> End {
        out.push(Effect::AllowSleep);
        out.push(Effect::Complete);
        End::Complete
    }
}

/// The runtime wrapper.
#[derive(Debug)]
pub enum Any {
    Idle(Idle),
    Checks(Checks),
    Configuration(Configuration),
    Evaluation(Evaluation),
    Build(Build),
    Cache(Cache),
    Prepared(Prepared),
    Cancelling(Cancelling),
    Erase(Erase),
    Format(Format),
    Files(Files),
    Copy(Copy),
    BuildOnTarget(BuildOnTarget),
    Bootloader(Bootloader),
    Record(Record),
    Flush(Flush),
    End(End),
}

pub struct Install {
    config: Config,
    state: Any,
}

impl Install {
    pub fn typed(&self) -> &Any {
        &self.state
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Self { config, state: Any::Idle(Idle(())) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use Any as A;
        use Event as E;
        let c = &self.config;
        let state = std::mem::replace(&mut self.state, A::End(End::Failed));
        self.state = match (state, event) {
            (A::Idle(s), E::Begin) => A::Checks(s.begin(out)),
            (A::Idle(s), E::Cancel) => A::End(s.cancel(out)),
            (A::Idle(s), E::Confirm { .. }) => A::End(s.confirm_early(out)),
            (A::Checks(s), E::Checked { root_mib }) => A::Configuration(s.checked(root_mib, out)),
            (A::Configuration(s), E::Configured) => A::Evaluation(s.configured(out)),
            (A::Evaluation(s), E::Evaluated { download_mib, unpacked_mib, budget_mib }) => {
                match s.evaluated(c, download_mib, unpacked_mib, budget_mib, out) {
                    Route::Build(b) => A::Build(b),
                    Route::Deferred(p) => A::Prepared(p),
                    Route::Refused(e) => A::End(e),
                }
            }
            (A::Build(s), E::Built { closure_mib }) => match s.built(c, closure_mib, out) {
                Ok(cache) => A::Cache(cache),
                Err(e) => A::End(e),
            },
            (A::Cache(s), E::Cached) => A::Prepared(s.cached(out)),
            (A::Checks(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Configuration(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Evaluation(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Build(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Cache(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Checks(s), E::Cancel) => A::Cancelling(s.cancel(out)),
            (A::Configuration(s), E::Cancel) => A::Cancelling(s.cancel(out)),
            (A::Evaluation(s), E::Cancel) => A::Cancelling(s.cancel(out)),
            (A::Build(s), E::Cancel) => A::Cancelling(s.cancel(out)),
            (A::Cache(s), E::Cancel) => A::Cancelling(s.cancel(out)),
            (A::Checks(s), E::Confirm { .. }) => A::End(s.confirm_early(out)),
            (A::Configuration(s), E::Confirm { .. }) => A::End(s.confirm_early(out)),
            (A::Evaluation(s), E::Confirm { .. }) => A::End(s.confirm_early(out)),
            (A::Build(s), E::Confirm { .. }) => A::End(s.confirm_early(out)),
            (A::Cache(s), E::Confirm { .. }) => A::End(s.confirm_early(out)),
            (
                A::Cancelling(s),
                E::Checked { .. }
                | E::Configured
                | E::Evaluated { .. }
                | E::Built { .. }
                | E::Cached
                | E::StepDone
                | E::StageFailed,
            ) => A::End(s.stopped(out)),
            (A::Prepared(s), E::Confirm { phrase_ok }) => match s.confirm(phrase_ok, out) {
                Ok(e) => A::Erase(e),
                Err(e) => A::End(e),
            },
            (A::Prepared(s), E::Cancel) => A::End(s.cancel(out)),
            (A::Erase(s), E::StepDone) => A::Format(s.done(out)),
            (A::Format(s), E::StepDone) => A::Files(s.done(out)),
            (A::Files(s), E::StepDone) => match s.done(out) {
                Step4::Copy(s) => A::Copy(s),
                Step4::BuildOnTarget(s) => A::BuildOnTarget(s),
            },
            (A::Copy(s), E::StepDone) => A::Bootloader(s.done(out)),
            (A::BuildOnTarget(s), E::StepDone) => A::Bootloader(s.done(out)),
            (A::Bootloader(s), E::StepDone) => A::Record(s.done(out)),
            (A::Record(s), E::StepDone) => A::Flush(s.done(out)),
            (A::Record(s), E::StageFailed) => A::Flush(s.failed(out)),
            (A::Flush(s), E::StepDone) => A::End(s.done(out)),
            (A::Erase(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Format(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Files(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Copy(s), E::StageFailed) => A::End(s.failed(out)),
            (A::BuildOnTarget(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Bootloader(s), E::StageFailed) => A::End(s.failed(out)),
            (A::Flush(s), E::StageFailed) => A::End(s.failed(out)),
            (s, _) => s,
        };
    }

    fn state(&self) -> StateName {
        match self.state {
            Any::Idle(_) => StateName::Idle,
            Any::Checks(_) => StateName::Checks,
            Any::Configuration(_) => StateName::Configuration,
            Any::Evaluation(_) => StateName::Evaluation,
            Any::Build(_) => StateName::Build,
            Any::Cache(_) => StateName::Cache,
            Any::Prepared(_) => StateName::Prepared,
            Any::Cancelling(_) => StateName::Cancelling,
            Any::Erase(_) => StateName::Erase,
            Any::Format(_) => StateName::Format,
            Any::Files(_) => StateName::Files,
            Any::Copy(_) => StateName::Copy,
            Any::BuildOnTarget(_) => StateName::BuildOnTarget,
            Any::Bootloader(_) => StateName::Bootloader,
            Any::Record(_) => StateName::Record,
            Any::Flush(_) => StateName::Flush,
            Any::End(End::Complete) => StateName::Complete,
            Any::End(End::Failed) => StateName::Failed,
            Any::End(End::Refused) => StateName::Refused,
            Any::End(End::Cancelled) => StateName::Cancelled,
        }
    }
}
