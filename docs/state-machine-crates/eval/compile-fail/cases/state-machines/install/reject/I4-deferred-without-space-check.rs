// I4 deferred-without-space-check: a deferred `Prepared` is reached only by
// `evaluated`'s candidate guarded by `fits_unpacked` (the room check); the
// in-memory route's `cached`, the only other way into `Prepared`, doesn't
// exist on `Installer<Evaluation>`.
use impl_state_machines::install::{Ctx, Installer};
use spec::install::{Config, GIB};

fn main() {
    let evaluation = Installer::new(Ctx::new(Config::default())).begin().unwrap().checked(39 * GIB).unwrap();
    let evaluation = evaluation.configured().unwrap();
    let _ = evaluation.cached();
}
