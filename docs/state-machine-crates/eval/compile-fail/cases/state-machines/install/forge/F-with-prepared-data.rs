// Forge (I4 deferred-without-space-check). The generated, public
// `with_prepared_data` turns the in-memory route's `Prepared` deferred: the
// installation then builds on the target (step 4 `BuildOnTarget`) though
// only the built closure's room was checked, never the unpacked downloads'.
// Other means that stay public: `<state>_data_mut` and the generic
// `state_data_<state>_mut` on every typed machine (the root's size, the
// route); `set_<state>_data` and `<state>_data_mut` on the wrapper
// `into_dynamic()` returns; and `initialize()` on every state type, which
// re-runs the active entry hooks: `Installer<Erase>::initialize()` emits
// StayAwake, Progress(1) and Run(Erase) again.
use impl_state_machines::install::{
    Ctx, Evaluated, Installer, InstallerBuildBuiltOutcome, InstallerEvaluationEvaluatedOutcome,
    InstallerFilesStepDoneOutcome, InstallerPreparedConfirmOutcome,
};
use spec::install::{Config, Effect, GIB, Stage};

fn main() {
    let evaluation = Installer::new(Ctx::new(Config::default())).begin().unwrap().checked(100 * GIB).unwrap();
    let evaluation = evaluation.configured().unwrap();
    let in_memory = Evaluated { download_mib: GIB, unpacked_mib: 300 * GIB, budget_mib: 2 * GIB };
    let InstallerEvaluationEvaluatedOutcome::Build(build) = evaluation.evaluated(in_memory).unwrap() else { panic!() };
    let InstallerBuildBuiltOutcome::Cache(cache) = build.built(10 * GIB).unwrap() else { panic!() };
    let prepared = cache.cached().unwrap().with_prepared_data(true);
    let InstallerPreparedConfirmOutcome::Erase(erase) = prepared.confirm(true).unwrap() else { panic!() };
    let files = erase.step_done().unwrap().step_done().unwrap();
    let InstallerFilesStepDoneOutcome::BuildOnTarget(mut step4) = files.step_done().unwrap() else { panic!() };
    let effects = step4.take_effects();
    assert!(effects.contains(&Effect::Run(Stage::BuildOnTarget)));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Refused { .. })));
}
