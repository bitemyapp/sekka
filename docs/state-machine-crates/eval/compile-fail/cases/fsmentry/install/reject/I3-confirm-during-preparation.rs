// I3 confirm-during-preparation: take the confirmation while a preparation
// stage runs. `confirm` is `Prepared`'s alone; `Preparing` has `prepared`,
// `refused`, `cancel` and `fail`.
use impl_fsmentry::install::{Install, SessionEntry, Steps};

fn confirm_early(m: &mut Install, steps: Steps) {
    if let SessionEntry::Preparing(preparing) = m.entry() {
        preparing.confirm(steps);
    }
}

fn main() {}
