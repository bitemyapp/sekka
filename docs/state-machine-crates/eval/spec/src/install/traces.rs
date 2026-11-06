//! Conformance traces for `install`. Sizes in MiB.

use super::{Config, Effect as F, Event as E, GIB, Stage, StateName as S, Warning};
use crate::testing::{Driven, Tape, Target, Trace};

/// The `install` target.
pub struct Spec;

impl<M: super::Machine> Driven<Spec> for M {
    fn boot(config: Config) -> Self {
        M::new(config)
    }
    fn step(&mut self, event: E, out: &mut Vec<F>) {
        self.handle(event, out);
    }
    fn observe(&self) -> S {
        self.state()
    }
}

impl Target for Spec {
    const NAME: &'static str = "install";
    type Config = Config;
    type Event = E;
    type Effect = F;
    type State = S;
    type Env = ();

    fn traces() -> Vec<Trace<Self>> {
        traces()
    }
    fn terminal(state: S) -> bool {
        matches!(state, S::Complete | S::Failed | S::Refused | S::Cancelled)
    }
    fn configs() -> Vec<Config> {
        vec![Config::default(), Config { reserve_mib: 0 }, Config { reserve_mib: 50 * GIB }]
    }
    fn env(_: &Config) {}
    fn next(_: &mut (), tape: &mut Tape, state: S, _: &[F]) -> E {
        super::env::event(tape, state)
    }
}

const ROOT: u64 = 100 * GIB;

/// Begin through `Evaluated`.
fn evaluated(
    t: Trace<Spec>,
    root_mib: u64,
    download_mib: u64,
    unpacked_mib: u64,
    budget_mib: u64,
    next: S,
    fx: Vec<F>,
) -> Trace<Spec> {
    t.step(E::Begin, [F::Run(Stage::Checks)], S::Checks)
        .step(E::Checked { root_mib }, [F::Run(Stage::Configuration)], S::Configuration)
        .step(E::Configured, [F::Run(Stage::Evaluation)], S::Evaluation)
        .step(E::Evaluated { download_mib, unpacked_mib, budget_mib }, fx, next)
}

/// The in-memory route through `Prepared` with a 12 GiB closure.
fn prepared(name: &'static str, about: &'static str) -> Trace<Spec> {
    evaluated(
        Trace::new(name, about, Config::default()),
        ROOT,
        2 * GIB,
        8 * GIB,
        30 * GIB,
        S::Build,
        vec![F::Run(Stage::Build)],
    )
    .step(E::Built { closure_mib: 12 * GIB }, [F::Run(Stage::Cache)], S::Cache)
    .step(E::Cached, [F::Prepared { deferred: false, closure_mib: 12 * GIB }], S::Prepared)
}

/// From `Prepared` (in-memory route) through step 3, `Files`.
fn to_files(t: Trace<Spec>) -> Trace<Spec> {
    t.step(E::Confirm { phrase_ok: true }, [F::StayAwake, F::Progress(1), F::Run(Stage::Erase)], S::Erase)
        .step(E::StepDone, [F::Progress(2), F::Run(Stage::Format)], S::Format)
        .step(E::StepDone, [F::Progress(3), F::Run(Stage::Files)], S::Files)
}

pub fn traces() -> Vec<Trace<Spec>> {
    let c = Config::default();
    vec![
        to_files(prepared("in-memory-route-installs", "build in memory, check the size, cache, confirm, steps 1-6"))
            .step(E::StepDone, [F::Progress(4), F::Run(Stage::Copy)], S::Copy)
            .step(E::StepDone, [F::Progress(5), F::Run(Stage::Bootloader)], S::Bootloader)
            .step(E::StepDone, [F::Run(Stage::Record)], S::Record)
            .step(E::StepDone, [F::Progress(6), F::Run(Stage::Flush)], S::Flush)
            .step(E::StepDone, [F::AllowSleep, F::Complete], S::Complete)
            .step(E::StepDone, [], S::Complete)
            .step(E::Cancel, [], S::Complete),
        evaluated(
            Trace::new("deferred-route-builds-on-the-target", "downloads beyond memory: build on the target after formatting", c),
            ROOT,
            40 * GIB,
            60 * GIB,
            30 * GIB,
            S::Prepared,
            vec![F::Prepared { deferred: true, closure_mib: 0 }],
        )
        .step(E::Confirm { phrase_ok: true }, [F::StayAwake, F::Progress(1), F::Run(Stage::Erase)], S::Erase)
        .step(E::StepDone, [F::Progress(2), F::Run(Stage::Format)], S::Format)
        .step(E::StepDone, [F::Progress(3), F::Run(Stage::Files)], S::Files)
        .step(E::StepDone, [F::Progress(4), F::Run(Stage::BuildOnTarget)], S::BuildOnTarget)
        .step(E::StepDone, [F::Progress(5), F::Run(Stage::Bootloader)], S::Bootloader)
        .step(E::StageFailed, [F::AllowSleep, F::Failed { erased: true }], S::Failed),
        evaluated(
            Trace::new("in-memory-route-refuses-a-small-disk", "the built closure plus the reserve doesn't fit: refuse before erasing", c),
            ROOT,
            2 * GIB,
            8 * GIB,
            30 * GIB,
            S::Build,
            vec![F::Run(Stage::Build)],
        )
        .step(E::Built { closure_mib: 95 * GIB }, [F::Refused { needed_mib: 103 * GIB, root_mib: ROOT }], S::Refused)
        .step(E::Confirm { phrase_ok: true }, [], S::Refused),
        Trace::new("cancel-during-preparation", "Cancel stops the stage; its end, even a failure, is a cancellation", c)
            .step(E::Begin, [F::Run(Stage::Checks)], S::Checks)
            .step(E::Checked { root_mib: ROOT }, [F::Run(Stage::Configuration)], S::Configuration)
            .step(E::Cancel, [F::Interrupt], S::Cancelling)
            .step(E::Cancel, [], S::Cancelling)
            .step(E::StageFailed, [F::Cancelled], S::Cancelled)
            .step(E::Configured, [], S::Cancelled),
        evaluated(Trace::new("cancel-during-the-build", "a cancelled build's late result is a cancellation", c), ROOT, 2 * GIB, 8 * GIB, 30 * GIB, S::Build, vec![F::Run(Stage::Build)])
            .step(E::Cancel, [F::Interrupt], S::Cancelling)
            .step(E::Confirm { phrase_ok: true }, [], S::Cancelling)
            .step(E::Built { closure_mib: GIB }, [F::Cancelled], S::Cancelled),
        prepared("cancel-after-prepared", "withdrawing at the review writes nothing")
            .step(E::Cancel, [F::Cancelled], S::Cancelled)
            .step(E::Confirm { phrase_ok: true }, [], S::Cancelled),
        to_files(prepared("cancel-after-the-first-write-is-ignored", "past the irreversible point the install runs to its end"))
            .step(E::Cancel, [], S::Files)
            .step(E::StepDone, [F::Progress(4), F::Run(Stage::Copy)], S::Copy)
            .step(E::Confirm { phrase_ok: true }, [], S::Copy),
        Trace::new("early-confirmation-is-refused", "a confirmation before the review is a protocol error: nothing is written", c)
            .step(E::Begin, [F::Run(Stage::Checks)], S::Checks)
            .step(E::Checked { root_mib: ROOT }, [F::Run(Stage::Configuration)], S::Configuration)
            .step(E::Confirm { phrase_ok: true }, [F::Interrupt, F::Failed { erased: false }], S::Failed)
            .step(E::Configured, [], S::Failed),
        Trace::new("confirmation-before-anything", "a confirmation with no request is refused too", c)
            .step(E::Confirm { phrase_ok: true }, [F::Failed { erased: false }], S::Failed)
            .step(E::Begin, [], S::Failed),
        prepared("wrong-confirmation-phrase", "a confirmation that doesn't match the plan fails without writing")
            .step(E::Confirm { phrase_ok: false }, [F::Failed { erased: false }], S::Failed),
        Trace::new("failure-before-erasing", "a preparation failure leaves the disk untouched", c)
            .step(E::Begin, [F::Run(Stage::Checks)], S::Checks)
            .step(E::Checked { root_mib: ROOT }, [F::Run(Stage::Configuration)], S::Configuration)
            .step(E::Configured, [F::Run(Stage::Evaluation)], S::Evaluation)
            .step(E::StageFailed, [F::Failed { erased: false }], S::Failed),
        prepared("failure-after-erasing", "a failure once the disk was written says so")
            .step(E::Confirm { phrase_ok: true }, [F::StayAwake, F::Progress(1), F::Run(Stage::Erase)], S::Erase)
            .step(E::StepDone, [F::Progress(2), F::Run(Stage::Format)], S::Format)
            .step(E::StageFailed, [F::AllowSleep, F::Failed { erased: true }], S::Failed),
        to_files(prepared("record-failure-isnt-fatal", "the bootloader is in: a missing install.json is only a warning"))
            .step(E::StepDone, [F::Progress(4), F::Run(Stage::Copy)], S::Copy)
            .step(E::StepDone, [F::Progress(5), F::Run(Stage::Bootloader)], S::Bootloader)
            .step(E::StepDone, [F::Run(Stage::Record)], S::Record)
            .step(E::StageFailed, [F::Warn(Warning::NoRecord), F::Progress(6), F::Run(Stage::Flush)], S::Flush)
            .step(E::StepDone, [F::AllowSleep, F::Complete], S::Complete),
        Trace::new("out-of-order-results-are-ignored", "only the running stage's result counts", c)
            .step(E::StepDone, [], S::Idle)
            .step(E::Checked { root_mib: ROOT }, [], S::Idle)
            .step(E::Begin, [F::Run(Stage::Checks)], S::Checks)
            .step(E::Begin, [], S::Checks)
            .step(E::Configured, [], S::Checks)
            .step(E::Built { closure_mib: GIB }, [], S::Checks)
            .step(E::StepDone, [], S::Checks)
            .step(E::Checked { root_mib: ROOT }, [F::Run(Stage::Configuration)], S::Configuration)
            .step(E::Cached, [], S::Configuration)
            .step(E::Evaluated { download_mib: 1, unpacked_mib: 1, budget_mib: 1 }, [], S::Configuration),
        Trace::new("cancel-before-begin", "withdrawn before the request: cancelled", c)
            .step(E::Cancel, [F::Cancelled], S::Cancelled)
            .step(E::Begin, [], S::Cancelled),
        // Confirmed bug GEN-07, as the correct behaviour.
        evaluated(
            Trace::new(
                "deferred-route-checks-the-root-space",
                "60 GiB of downloads, 300 GiB unpacked, a 39 GiB root: refuse before erasing, as the in-memory route would",
                c,
            )
            .bug("GEN-07"),
            39 * GIB,
            60 * GIB,
            300 * GIB,
            29 * GIB,
            S::Refused,
            vec![F::Refused { needed_mib: 308 * GIB, root_mib: 39 * GIB }],
        )
        .step(E::Confirm { phrase_ok: true }, [], S::Refused),
    ]
}
