// T4 place-without-answer: over a conflict the job is a typed
// `Job<Asking>`, which has only `answered`: no `placed` without the
// person's answer.
use impl_state_machines::transfer::{Ctx, Job, JobIdleStartOutcome, JobProbingProbedOutcome};
use spec::transfer::{Config, Found, ItemId, Placed};

fn main() {
    let item = ItemId(0);
    let JobIdleStartOutcome::Probing(probing) = Job::new(Ctx::new(Config::default())).start(1).unwrap() else {
        return;
    };
    let conflict = Found::Conflict { can_merge: true };
    let JobProbingProbedOutcome::AskingMergeable(asking) = probing.probed((item, conflict)).unwrap() else { return };
    let _ = asking.placed((item, Placed::Ok));
}
