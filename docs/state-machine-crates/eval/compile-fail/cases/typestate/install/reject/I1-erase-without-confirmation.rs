// I1 erase-without-confirmation: start the destructive steps from
// `Prepared` without the confirmation. `Prepared::confirm` is the only way
// to an `Erase`.
use impl_typestate::install::{Idle, Route};
use spec::install::{Config, Effect, GIB};

fn main() {
    let c = Config::default();
    let mut out: Vec<Effect> = Vec::new();
    let evaluation = Idle::new().begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let Route::Deferred(prepared) = evaluation.evaluated(&c, 2 * GIB, 10 * GIB, GIB, &mut out) else {
        return;
    };
    let _ = prepared.erase(&mut out);
}
