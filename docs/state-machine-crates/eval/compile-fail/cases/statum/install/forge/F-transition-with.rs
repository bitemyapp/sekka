// Forge, I4 deferred-without-space-check (GEN-07). The private `deferred`
// declares the edge `Evaluation → Prepared` (private `#[transition]` methods
// get the public impls too), so statum implements the public
// `CanTransitionWith<PreparedData>` for `Helper<Evaluation>`, and taking it
// skips the check. Outside code can't make a `PreparedData` (private field),
// but `state_data` is always a public field: it moves one out of another
// helper whose large root passed the check. A 39 GiB root then reaches
// `Prepared` for a selection that unpacks to 300 GiB, and the confirmation
// erases the disk. (The other public means: `transition_map` and
// `CanTransitionMap`; I1–I3 the same way, with an `EraseData`, a `CopyTok` or
// a `PreparedData` from a donor.)
use impl_statum::install::{Helper, Idle, Prepared, Route};
use spec::install::{Config, Effect, GIB, Stage};
use statum::CanTransitionWith;

fn main() {
    let mut donor_out: Vec<Effect> = Vec::new();
    let donor = Helper::<Idle>::new(Config::default())
        .begin(&mut donor_out)
        .checked(400 * GIB, &mut donor_out)
        .configured(&mut donor_out);
    let Route::Deferred(donor) = donor.evaluated(60 * GIB, 300 * GIB, 2 * GIB, &mut donor_out) else {
        panic!("donor refused");
    };
    let mut out: Vec<Effect> = Vec::new();
    let evaluation =
        Helper::<Idle>::new(Config::default()).begin(&mut out).checked(39 * GIB, &mut out).configured(&mut out);
    let prepared: Helper<Prepared> = evaluation.transition_with_data(donor.state_data);
    let Ok(_erase) = prepared.confirm(true, &mut out) else { panic!("not erasing") };
    assert!(out.contains(&Effect::Run(Stage::Erase)));
    assert!(!out.iter().any(|e| matches!(e, Effect::Refused { .. })));
}
