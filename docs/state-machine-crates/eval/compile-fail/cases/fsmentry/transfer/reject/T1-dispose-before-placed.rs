// T1 dispose-before-placed: placing the replacement failed; dispose of the
// aside anyway. Only `PlacingReplacement::placed` (the replacement is in
// place) leads to `Disposing`; `Restoring` leads only on, by `restored_*`.
use impl_fsmentry::transfer::{JobEntry, Replaced, Restore, Transfer};

fn dispose_anyway(m: &mut Transfer, restore: Restore, replaced: Replaced) {
    if let JobEntry::PlacingReplacement(replacement) = m.entry() {
        replacement.failed(restore);
    }
    if let JobEntry::Restoring(restoring) = m.entry() {
        restoring.placed(replaced);
    }
}

fn main() {}
