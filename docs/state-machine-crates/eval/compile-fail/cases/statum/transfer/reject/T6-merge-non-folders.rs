// T6 merge-non-folders: merge a conflict that isn't folder onto folder. Such
// a conflict is `Job<AskingFiles>`; only `Job<AskingFolders>` has `merge`.
use impl_statum::transfer::{AskingFiles, Job};
use spec::transfer::Effect;

fn merge(asking: Job<AskingFiles>, out: &mut Vec<Effect>) {
    let _ = asking.merge(out);
}

fn main() {}
