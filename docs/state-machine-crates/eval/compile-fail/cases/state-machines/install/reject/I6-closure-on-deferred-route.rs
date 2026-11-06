// I6 closure-on-deferred-route: the closure's size is `Cache`'s data, and
// its typed accessor exists only on `Installer<Cache>`; the deferred
// route's `Prepared` carries only the route. (The generic
// `state_data_cache()` exists on every state type and returns `None` here.)
use impl_state_machines::install::{Ctx, Evaluated, Installer, InstallerEvaluationEvaluatedOutcome};
use spec::install::{Config, GIB};

fn main() {
    let evaluation = Installer::new(Ctx::new(Config::default())).begin().unwrap().checked(100 * GIB).unwrap().configured().unwrap();
    let deferred = Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB };
    let InstallerEvaluationEvaluatedOutcome::Prepared(prepared) = evaluation.evaluated(deferred).unwrap() else { return };
    let _ = prepared.cache_data();
}
