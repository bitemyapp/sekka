// T1 dispose-before-placed: placing the replacement failed; dispose of the
// aside anyway. Only `PlacingReplacement::placed` (the replacement is in
// place) trashes or deletes it; `Restoring` can only put it back.
use impl_typestate::transfer::{Disposing, Job, PlacingReplacement};
use spec::transfer::Effect;

fn dispose_anyway(job: &Job, replacement: PlacingReplacement, out: &mut Vec<Effect>) -> Disposing {
    let restoring = replacement.failed(false, out);
    restoring.placed(job, out)
}

fn main() {}
