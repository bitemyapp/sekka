// I1 erase-without-confirmation: `Erase` is entered only by `confirm` from
// `Prepared`; the typed `Installer<Prepared>` has no `step_done` (nor
// any other way into `Executing`).
use impl_state_machines::install::{Ctx, Evaluated, Installer, InstallerEvaluationEvaluatedOutcome};
use spec::install::{Config, GIB};

fn main() {
    let evaluation = Installer::new(Ctx::new(Config::default())).begin().unwrap().checked(100 * GIB).unwrap().configured().unwrap();
    let deferred = Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB };
    let InstallerEvaluationEvaluatedOutcome::Prepared(prepared) = evaluation.evaluated(deferred).unwrap() else { return };
    let _ = prepared.step_done();
}
