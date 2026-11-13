// I2 bootloader-before-copy: `step_done` from `Files` has only step 4
// (`BuildOnTarget` or `Copy`) among its typed outcomes.
use impl_state_machines::install::{
    Ctx, Evaluated, Installer, InstallerEvaluationEvaluatedOutcome, InstallerFilesStepDoneOutcome,
    InstallerPreparedConfirmOutcome,
};
use spec::install::{Config, GIB};

fn main() {
    let evaluation = Installer::new(Ctx::new(Config::default())).begin().unwrap().checked(100 * GIB).unwrap().configured().unwrap();
    let deferred = Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB };
    let InstallerEvaluationEvaluatedOutcome::Prepared(prepared) = evaluation.evaluated(deferred).unwrap() else { return };
    let InstallerPreparedConfirmOutcome::Erase(erase) = prepared.confirm(true).unwrap() else { return };
    let files = erase.step_done().unwrap().step_done().unwrap();
    match files.step_done().unwrap() {
        InstallerFilesStepDoneOutcome::Bootloader(_early) => {}
        _ => {}
    }
}
