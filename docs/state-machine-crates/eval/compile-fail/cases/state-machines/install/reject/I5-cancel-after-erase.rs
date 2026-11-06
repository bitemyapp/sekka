// I5 cancel-after-erase: `cancel` is declared from `Idle`, `Prepared`
// and `Preparing`; the typed `Installer<Erase>` has no such method.
use impl_state_machines::install::{Ctx, Evaluated, Installer, InstallerEvaluationEvaluatedOutcome, InstallerPreparedConfirmOutcome};
use spec::install::{Config, GIB};

fn main() {
    let evaluation = Installer::new(Ctx::new(Config::default())).begin().unwrap().checked(100 * GIB).unwrap().configured().unwrap();
    let deferred = Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB };
    let InstallerEvaluationEvaluatedOutcome::Prepared(prepared) = evaluation.evaluated(deferred).unwrap() else { return };
    let InstallerPreparedConfirmOutcome::Erase(erase) = prepared.confirm(true).unwrap() else { return };
    let _ = erase.cancel();
}
