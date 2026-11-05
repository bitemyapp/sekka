// T1 dispose-before-placed: placing the replacement failed; dispose of the
// aside anyway. Only `Job<PlacingReplacement>::placed` (the replacement is in
// place) trashes or deletes it; `Job<Restoring>` can only put it back.
use impl_statum::transfer::{Disposing, Job, PlacingReplacement};
use spec::transfer::Effect;

fn dispose_anyway(replacement: Job<PlacingReplacement>, out: &mut Vec<Effect>) -> Job<Disposing> {
    let restoring = replacement.failed(false, out);
    restoring.placed(out)
}

fn main() {}
