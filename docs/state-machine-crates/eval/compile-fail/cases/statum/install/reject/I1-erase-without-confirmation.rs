// I1 erase-without-confirmation: start the destructive steps from `Prepared`
// without the confirmation. `Helper<Prepared>::confirm` (the Confirm event,
// with its phrase check) is the only way to a `Helper<Erase>`.
use impl_statum::install::{Helper, Idle, Route};
use spec::install::{Config, Effect, GIB};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let evaluation =
        Helper::<Idle>::new(Config::default()).begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let Route::Deferred(prepared) = evaluation.evaluated(2 * GIB, 10 * GIB, GIB, &mut out) else { return };
    let _ = prepared.erase(&mut out);
}
