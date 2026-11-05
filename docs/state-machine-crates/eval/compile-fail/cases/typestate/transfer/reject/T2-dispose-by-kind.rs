// T2 dispose-by-kind: choose Delete for a `Restore` job (the current code's
// `Worker { trash: None }`). The `Job` keeps the mode and the person's
// `delete_replaced`, not the kind, and `PlacingReplacement::placed` reads
// only the latter.
use impl_typestate::transfer::{Disposing, Job, PlacingReplacement};
use spec::transfer::{Effect, JobKind};

fn dispose(job: &Job, replacement: PlacingReplacement, out: &mut Vec<Effect>) -> Disposing {
    if job.kind == JobKind::Restore {
        // ... delete instead of trashing
    }
    replacement.placed(job, out)
}

fn main() {}
