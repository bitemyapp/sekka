// I5 cancel-after-erase: cancel once the disk is being erased. The
// destructive steps have `done` and `failed`, no `cancel`.
use impl_typestate::install::{Idle, Route};
use spec::install::{Config, Effect, GIB};

fn main() {
    let c = Config::default();
    let mut out: Vec<Effect> = Vec::new();
    let evaluation = Idle::new().begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let Route::Deferred(prepared) = evaluation.evaluated(&c, 2 * GIB, 10 * GIB, GIB, &mut out) else {
        return;
    };
    let Ok(erase) = prepared.confirm(true, &mut out) else { return };
    let _ = erase.cancel(&mut out);
}
