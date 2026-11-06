// I6 closure-on-deferred-route: read the closure's size from the deferred
// route's `Prepared`. Its state data (`PreparedData`) holds only the route;
// only `Cache` (in-memory route) holds a closure.
use impl_statum::install::{Helper, Idle, Route};
use spec::install::{Config, Effect, GIB};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let evaluation =
        Helper::<Idle>::new(Config::default()).begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let Route::Deferred(prepared) = evaluation.evaluated(2 * GIB, 10 * GIB, GIB, &mut out) else { return };
    let _ = prepared.state_data.closure;
}
