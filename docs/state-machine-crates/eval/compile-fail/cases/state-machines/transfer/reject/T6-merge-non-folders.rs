// T6 merge-non-folders: `Asking` is split by the conflict's `can_merge`, so
// a conflict that can't merge is a `Job<AskingUnmergeable>`, whose typed
// `answered` outcomes have no `Listing` (a Merge there is a replace).
use impl_state_machines::transfer::{
    Ctx, Job, JobAskingUnmergeableAnsweredOutcome, JobIdleStartOutcome, JobProbingProbedOutcome,
};
use spec::transfer::{Config, Found, ItemId, Resolution};

fn main() {
    let item = ItemId(0);
    let JobIdleStartOutcome::Probing(probing) = Job::new(Ctx::new(Config::default())).start(1).unwrap() else {
        return;
    };
    let conflict = Found::Conflict { can_merge: false };
    let JobProbingProbedOutcome::AskingUnmergeable(asking) = probing.probed((item, conflict)).unwrap() else { return };
    match asking.answered((item, Resolution::Merge)).unwrap() {
        JobAskingUnmergeableAnsweredOutcome::Listing(_merging) => {}
        _ => {}
    }
}
