// T6 merge-non-folders: merge a conflict that isn't folder onto folder.
// Such a conflict is `AskingFiles`; only `AskingFolders` has `merge`.
use impl_typestate::transfer::{AskingFiles, Listing};
use spec::transfer::Effect;

fn merge(asking: AskingFiles, out: &mut Vec<Effect>) -> Listing {
    asking.merge(out)
}

fn main() {}
