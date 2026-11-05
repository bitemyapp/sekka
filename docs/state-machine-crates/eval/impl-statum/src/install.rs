//! `install` with statum 0.9.0: one `#[state]` per stage (what later stages
//! need in sealed payloads, tokens for the rest), the reserve as the
//! `#[machine] Helper`'s field.
//! `Prepared` is only reached by the checks both routes run, the
//! destructive steps start only from `Prepared::confirm`, each step has only
//! its successor, and nothing past `Erase` can be cancelled. statum has no
//! superstates: `preparing!` and `executing!` give the preparation stages
//! and the destructive steps their shared edges.

use crate::{SEAL, Seal};
use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};
use statum::{Branch, machine, state, transition};

pub const IMPLEMENTED: bool = true;

tokens!(
    IdleTok,
    ChecksTok,
    CancellingTok,
    CopyTok,
    BuildOnTargetTok,
    BootloaderTok,
    RecordTok,
    FlushTok,
    CompleteTok,
    FailedTok,
    RefusedTok,
    CancelledTok,
);

/// The data states' payloads, sealed: only this module makes them.
macro_rules! payloads {
    ($($d:ident { $f:ident: $t:ty }),* $(,)?) => {$(
        #[derive(Debug)]
        pub struct $d {
            $f: $t,
        }
    )*};
}
payloads!(
    ConfigurationData { root: u64 },
    EvaluationData { root: u64 },
    BuildData { root: u64 },
    CacheData { closure: u64 },
    PreparedData { deferred: bool },
    EraseData { deferred: bool },
    FormatData { deferred: bool },
    FilesData { deferred: bool },
);

#[state]
#[derive(Debug)]
pub enum InstallState {
    Idle(IdleTok),
    Checks(ChecksTok),
    Configuration(ConfigurationData),
    Evaluation(EvaluationData),
    Build(BuildData),
    Cache(CacheData),
    Prepared(PreparedData),
    Cancelling(CancellingTok),
    /// Steps 1–3 carry the route to step 4.
    Erase(EraseData),
    Format(FormatData),
    Files(FilesData),
    Copy(CopyTok),
    BuildOnTarget(BuildOnTargetTok),
    Bootloader(BootloaderTok),
    Record(RecordTok),
    Flush(FlushTok),
    Complete(CompleteTok),
    Failed(FailedTok),
    Refused(RefusedTok),
    Cancelled(CancelledTok),
}

#[machine]
#[derive(Debug)]
pub struct Helper<InstallState> {
    reserve_mib: u64,
    seal: Seal,
}

fn run(stage: Stage, out: &mut impl Sink<Effect>) {
    out.push(Effect::Run(stage));
}

fn step(n: u8, stage: Stage, out: &mut impl Sink<Effect>) {
    out.push(Effect::Progress(n));
    run(stage, out);
}

impl Helper<Idle> {
    #[inline]
    pub fn new(config: Config) -> Self {
        Helper::<Idle>::builder().state_data(IdleTok(())).reserve_mib(config.reserve_mib).seal(SEAL).build()
    }
}

#[transition]
impl Helper<Idle> {
    pub fn begin(self, out: &mut impl Sink<Effect>) -> Helper<Checks> {
        run(Stage::Checks, out);
        self.transition_with(ChecksTok(()))
    }
    pub fn cancel(self, out: &mut impl Sink<Effect>) -> Helper<Cancelled> {
        out.push(Effect::Cancelled);
        self.transition_with(CancelledTok(()))
    }
    pub fn confirm_early(self, out: &mut impl Sink<Effect>) -> Helper<Failed> {
        out.push(Effect::Failed { erased: false });
        self.transition_with(FailedTok(()))
    }
}

/// What preparation stages share: a failure, a cancel, an early confirmation.
macro_rules! preparing {
    ($($s:ident),*) => {$(
        #[transition]
        impl Helper<$s> {
            pub fn failed(self, out: &mut impl Sink<Effect>) -> Helper<Failed> {
                out.push(Effect::Failed { erased: false });
                self.transition_with(FailedTok(()))
            }
            pub fn cancel(self, out: &mut impl Sink<Effect>) -> Helper<Cancelling> {
                out.push(Effect::Interrupt);
                self.transition_with(CancellingTok(()))
            }
            /// A confirmation before the review is a protocol error.
            pub fn confirm_early(self, out: &mut impl Sink<Effect>) -> Helper<Failed> {
                out.push(Effect::Interrupt);
                self.failed(out)
            }
        }
    )*};
}
preparing!(Checks, Configuration, Evaluation, Build, Cache);

/// What destructive steps share: a failure.
macro_rules! executing {
    ($($s:ident),*) => {$(
        #[transition]
        impl Helper<$s> {
            pub fn failed(self, out: &mut impl Sink<Effect>) -> Helper<Failed> {
                out.push(Effect::AllowSleep);
                out.push(Effect::Failed { erased: true });
                self.transition_with(FailedTok(()))
            }
        }
    )*};
}
executing!(Erase, Format, Files, Copy, BuildOnTarget, Bootloader, Flush);

#[transition]
impl Helper<Checks> {
    pub fn checked(self, root_mib: u64, out: &mut impl Sink<Effect>) -> Helper<Configuration> {
        run(Stage::Configuration, out);
        self.transition_with(ConfigurationData { root: root_mib })
    }
}

#[transition]
impl Helper<Configuration> {
    pub fn configured(self, out: &mut impl Sink<Effect>) -> Helper<Evaluation> {
        run(Stage::Evaluation, out);
        self.transition_map(|ConfigurationData { root }| EvaluationData { root })
    }
}

/// Where the evaluation leads (three ways: more than `Branch` holds).
pub enum Route {
    Build(Helper<Build>),
    Deferred(Helper<Prepared>),
    Refused(Helper<Refused>),
}

#[transition]
impl Helper<Evaluation> {
    fn build(self, out: &mut impl Sink<Effect>) -> Helper<Build> {
        run(Stage::Build, out);
        self.transition_map(|EvaluationData { root }| BuildData { root })
    }
    /// The deferred route checks the room for the unpacked downloads.
    fn deferred(
        self,
        unpacked_mib: u64,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Helper<Prepared>, Helper<Refused>> {
        let (needed, root_mib) = (unpacked_mib.saturating_add(self.reserve_mib), self.state_data.root);
        if root_mib < needed {
            out.push(Effect::Refused { needed_mib: needed, root_mib });
            return Err(self.transition_with(RefusedTok(())));
        }
        out.push(Effect::Prepared { deferred: true, closure_mib: 0 });
        Ok(self.transition_with(PreparedData { deferred: true }))
    }
}

impl Helper<Evaluation> {
    pub fn evaluated(
        self,
        download_mib: u64,
        unpacked_mib: u64,
        budget_mib: u64,
        out: &mut impl Sink<Effect>,
    ) -> Route {
        if download_mib <= budget_mib {
            return Route::Build(self.build(out));
        }
        match self.deferred(unpacked_mib, out) {
            Ok(p) => Route::Deferred(p),
            Err(r) => Route::Refused(r),
        }
    }
}

#[transition]
impl Helper<Build> {
    pub fn built(
        self,
        closure_mib: u64,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Helper<Cache>, Helper<Refused>> {
        let (needed, root_mib) = (closure_mib.saturating_add(self.reserve_mib), self.state_data.root);
        if root_mib < needed {
            out.push(Effect::Refused { needed_mib: needed, root_mib });
            return Err(self.transition_with(RefusedTok(())));
        }
        run(Stage::Cache, out);
        Ok(self.transition_with(CacheData { closure: closure_mib }))
    }
}

#[transition]
impl Helper<Cache> {
    pub fn cached(self, out: &mut impl Sink<Effect>) -> Helper<Prepared> {
        out.push(Effect::Prepared { deferred: false, closure_mib: self.state_data.closure });
        self.transition_with(PreparedData { deferred: false })
    }
}

#[transition]
impl Helper<Prepared> {
    /// The irreversible point: from here on the disk is written.
    pub fn confirm(
        self,
        phrase_ok: bool,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Helper<Erase>, Helper<Failed>> {
        if !phrase_ok {
            out.push(Effect::Failed { erased: false });
            return Err(self.transition_with(FailedTok(())));
        }
        out.push(Effect::StayAwake);
        step(1, Stage::Erase, out);
        Ok(self.transition_map(|PreparedData { deferred }| EraseData { deferred }))
    }
    pub fn cancel(self, out: &mut impl Sink<Effect>) -> Helper<Cancelled> {
        out.push(Effect::Cancelled);
        self.transition_with(CancelledTok(()))
    }
}

#[transition]
impl Helper<Cancelling> {
    /// The interrupted stage ended, whatever it reported.
    pub fn stopped(self, out: &mut impl Sink<Effect>) -> Helper<Cancelled> {
        out.push(Effect::Cancelled);
        self.transition_with(CancelledTok(()))
    }
}

#[transition]
impl Helper<Erase> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Helper<Format> {
        step(2, Stage::Format, out);
        self.transition_map(|EraseData { deferred }| FormatData { deferred })
    }
}

#[transition]
impl Helper<Format> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Helper<Files> {
        step(3, Stage::Files, out);
        self.transition_map(|FormatData { deferred }| FilesData { deferred })
    }
}

/// Step 4, by route.
pub type Step4 = ::statum::Branch<Helper<Copy>, Helper<BuildOnTarget>>;

#[transition]
impl Helper<Files> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Step4 {
        if self.state_data.deferred {
            step(4, Stage::BuildOnTarget, out);
            Branch::Second(self.transition_with(BuildOnTargetTok(())))
        } else {
            step(4, Stage::Copy, out);
            Branch::First(self.transition_with(CopyTok(())))
        }
    }
}

#[transition]
impl Helper<Copy> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Helper<Bootloader> {
        step(5, Stage::Bootloader, out);
        self.transition_with(BootloaderTok(()))
    }
}

#[transition]
impl Helper<BuildOnTarget> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Helper<Bootloader> {
        step(5, Stage::Bootloader, out);
        self.transition_with(BootloaderTok(()))
    }
}

#[transition]
impl Helper<Bootloader> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Helper<Record> {
        run(Stage::Record, out);
        self.transition_with(RecordTok(()))
    }
}

#[transition]
impl Helper<Record> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Helper<Flush> {
        step(6, Stage::Flush, out);
        self.transition_with(FlushTok(()))
    }
    /// The system is installed: a missing record is only a warning.
    pub fn failed(self, out: &mut impl Sink<Effect>) -> Helper<Flush> {
        out.push(Effect::Warn(Warning::NoRecord));
        self.done(out)
    }
}

#[transition]
impl Helper<Flush> {
    pub fn done(self, out: &mut impl Sink<Effect>) -> Helper<Complete> {
        out.push(Effect::AllowSleep);
        out.push(Effect::Complete);
        self.transition_with(CompleteTok(()))
    }
}

fn either<A, B>(r: Result<A, B>, a: fn(A) -> helper::SomeState, b: fn(B) -> helper::SomeState) -> helper::SomeState {
    match r {
        Ok(x) => a(x),
        Err(x) => b(x),
    }
}

pub struct Install {
    /// `None` only while `handle` has moved the state out (statum's builder
    /// makes no cheap placeholder).
    state: Option<helper::SomeState>,
}

impl Install {
    pub fn typed(&self) -> &helper::SomeState {
        self.state.as_ref().expect("a state")
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Self { state: Some(helper::SomeState::Idle(Helper::<Idle>::new(config))) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use Event as E;
        use helper::SomeState as S;
        let state = self.state.take().expect("a state");
        self.state = Some(match (state, event) {
            (S::Idle(s), E::Begin) => S::Checks(s.begin(out)),
            (S::Idle(s), E::Cancel) => S::Cancelled(s.cancel(out)),
            (S::Idle(s), E::Confirm { .. }) => S::Failed(s.confirm_early(out)),
            (S::Checks(s), E::Checked { root_mib }) => S::Configuration(s.checked(root_mib, out)),
            (S::Configuration(s), E::Configured) => S::Evaluation(s.configured(out)),
            (S::Evaluation(s), E::Evaluated { download_mib, unpacked_mib, budget_mib }) => {
                match s.evaluated(download_mib, unpacked_mib, budget_mib, out) {
                    Route::Build(b) => S::Build(b),
                    Route::Deferred(p) => S::Prepared(p),
                    Route::Refused(r) => S::Refused(r),
                }
            }
            (S::Build(s), E::Built { closure_mib }) => either(s.built(closure_mib, out), S::Cache, S::Refused),
            (S::Cache(s), E::Cached) => S::Prepared(s.cached(out)),
            (S::Checks(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Configuration(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Evaluation(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Build(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Cache(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Checks(s), E::Cancel) => S::Cancelling(s.cancel(out)),
            (S::Configuration(s), E::Cancel) => S::Cancelling(s.cancel(out)),
            (S::Evaluation(s), E::Cancel) => S::Cancelling(s.cancel(out)),
            (S::Build(s), E::Cancel) => S::Cancelling(s.cancel(out)),
            (S::Cache(s), E::Cancel) => S::Cancelling(s.cancel(out)),
            (S::Checks(s), E::Confirm { .. }) => S::Failed(s.confirm_early(out)),
            (S::Configuration(s), E::Confirm { .. }) => S::Failed(s.confirm_early(out)),
            (S::Evaluation(s), E::Confirm { .. }) => S::Failed(s.confirm_early(out)),
            (S::Build(s), E::Confirm { .. }) => S::Failed(s.confirm_early(out)),
            (S::Cache(s), E::Confirm { .. }) => S::Failed(s.confirm_early(out)),
            (
                S::Cancelling(s),
                E::Checked { .. }
                | E::Configured
                | E::Evaluated { .. }
                | E::Built { .. }
                | E::Cached
                | E::StepDone
                | E::StageFailed,
            ) => S::Cancelled(s.stopped(out)),
            (S::Prepared(s), E::Confirm { phrase_ok }) => either(s.confirm(phrase_ok, out), S::Erase, S::Failed),
            (S::Prepared(s), E::Cancel) => S::Cancelled(s.cancel(out)),
            (S::Erase(s), E::StepDone) => S::Format(s.done(out)),
            (S::Format(s), E::StepDone) => S::Files(s.done(out)),
            (S::Files(s), E::StepDone) => match s.done(out) {
                Branch::First(s) => S::Copy(s),
                Branch::Second(s) => S::BuildOnTarget(s),
            },
            (S::Copy(s), E::StepDone) => S::Bootloader(s.done(out)),
            (S::BuildOnTarget(s), E::StepDone) => S::Bootloader(s.done(out)),
            (S::Bootloader(s), E::StepDone) => S::Record(s.done(out)),
            (S::Record(s), E::StepDone) => S::Flush(s.done(out)),
            (S::Record(s), E::StageFailed) => S::Flush(s.failed(out)),
            (S::Flush(s), E::StepDone) => S::Complete(s.done(out)),
            (S::Erase(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Format(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Files(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Copy(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::BuildOnTarget(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Bootloader(s), E::StageFailed) => S::Failed(s.failed(out)),
            (S::Flush(s), E::StageFailed) => S::Failed(s.failed(out)),
            (s, _) => s,
        });
    }

    fn state(&self) -> StateName {
        use helper::SomeState as S;
        match self.typed() {
            S::Idle(_) => StateName::Idle,
            S::Checks(_) => StateName::Checks,
            S::Configuration(_) => StateName::Configuration,
            S::Evaluation(_) => StateName::Evaluation,
            S::Build(_) => StateName::Build,
            S::Cache(_) => StateName::Cache,
            S::Prepared(_) => StateName::Prepared,
            S::Cancelling(_) => StateName::Cancelling,
            S::Erase(_) => StateName::Erase,
            S::Format(_) => StateName::Format,
            S::Files(_) => StateName::Files,
            S::Copy(_) => StateName::Copy,
            S::BuildOnTarget(_) => StateName::BuildOnTarget,
            S::Bootloader(_) => StateName::Bootloader,
            S::Record(_) => StateName::Record,
            S::Flush(_) => StateName::Flush,
            S::Complete(_) => StateName::Complete,
            S::Failed(_) => StateName::Failed,
            S::Refused(_) => StateName::Refused,
            S::Cancelled(_) => StateName::Cancelled,
        }
    }
}
