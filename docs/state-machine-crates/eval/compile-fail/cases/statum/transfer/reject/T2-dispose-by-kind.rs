// T2 dispose-by-kind: choose Delete for a `Restore` job (the current code's
// `Worker { trash: None }`). The `#[machine] Job` keeps the mode and the
// person's `delete_replaced`, not the kind, and
// `Job<PlacingReplacement>::placed` reads only the latter.
use impl_statum::transfer::{Disposing, Job, PlacingReplacement};
use spec::transfer::{Effect, JobKind};

fn dispose(replacement: Job<PlacingReplacement>, out: &mut Vec<Effect>) -> Job<Disposing> {
    if replacement.kind == JobKind::Restore {
        // ... delete instead of trashing
    }
    replacement.placed(out)
}

fn main() {}
