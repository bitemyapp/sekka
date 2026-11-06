// I3 confirm-during-preparation: take the confirmation while evaluating.
// Only `Prepared` has `confirm`; a preparation stage has `confirm_early`,
// which fails the session.
use impl_typestate::install::Idle;
use spec::install::{Effect, GIB};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let evaluation = Idle::new().begin(&mut out).checked(100 * GIB, &mut out).configured(&mut out);
    let _ = evaluation.confirm(true, &mut out);
}
