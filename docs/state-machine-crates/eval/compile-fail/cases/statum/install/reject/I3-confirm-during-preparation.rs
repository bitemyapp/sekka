// I3 confirm-during-preparation: take the confirmation while evaluating.
// Only `Helper<Prepared>` has `confirm`; a preparation stage has
// `confirm_early`, which fails the session.
use impl_statum::install::{Helper, Idle};
use spec::install::{Config, Effect, GIB};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let evaluation =
        Helper::<Idle>::new(Config::default()).begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let _ = evaluation.confirm(true, &mut out);
}
