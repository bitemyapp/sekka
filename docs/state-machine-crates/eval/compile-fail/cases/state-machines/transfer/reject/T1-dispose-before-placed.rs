// T1 dispose-before-placed: the aside is disposed of by the `placed` Ok of
// `PlacingReplacement`; once placing the replacement failed, the job is a
// typed `Job<Restoring>`, which has no `placed` to dispose of it with.
use impl_state_machines::transfer::{
    Ctx, Job, JobAskingUnmergeableAnsweredOutcome, JobIdleStartOutcome, JobPlacingReplacementPlacedOutcome,
    JobProbingProbedOutcome, JobSettingAsideSetAsideOutcome,
};
use spec::transfer::{AsideId, Config, Found, ItemId, Placed, Resolution};

fn main() {
    let item = ItemId(0);
    let JobIdleStartOutcome::Probing(probing) = Job::new(Ctx::new(Config::default())).start(1).unwrap() else {
        return;
    };
    let conflict = Found::Conflict { can_merge: false };
    let JobProbingProbedOutcome::AskingUnmergeable(asking) = probing.probed((item, conflict)).unwrap() else { return };
    let JobAskingUnmergeableAnsweredOutcome::SettingAside(setting) = asking.answered((item, Resolution::Replace)).unwrap()
    else {
        return;
    };
    let JobSettingAsideSetAsideOutcome::PlacingReplacement(replacing) = setting.set_aside((item, Some(AsideId(5)))).unwrap()
    else {
        return;
    };
    let JobPlacingReplacementPlacedOutcome::Restoring(restoring) = replacing.placed((item, Placed::Skip)).unwrap() else {
        return;
    };
    let _ = restoring.placed((item, Placed::Ok));
}
