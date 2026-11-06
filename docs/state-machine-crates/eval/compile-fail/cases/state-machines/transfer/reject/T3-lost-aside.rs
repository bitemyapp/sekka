// T3 lost-aside: a typed `Job<Restoring>` has only `restored` (and the
// generated queries); there is no `probed` to move on to the next item
// with the aside left behind.
use impl_state_machines::transfer::{
    Ctx, Job, JobAskingUnmergeableAnsweredOutcome, JobIdleStartOutcome, JobPlacingReplacementPlacedOutcome, JobProbingProbedOutcome,
    JobSettingAsideSetAsideOutcome,
};
use spec::transfer::{AsideId, Config, Found, ItemId, Placed, Resolution};

fn main() {
    let item = ItemId(0);
    let JobIdleStartOutcome::Probing(probing) = Job::new(Ctx::new(Config::default())).start(2).unwrap() else {
        return;
    };
    let conflict = Found::Conflict { can_merge: false };
    let JobProbingProbedOutcome::AskingUnmergeable(asking) = probing.probed((item, conflict)).unwrap() else { return };
    let JobAskingUnmergeableAnsweredOutcome::SettingAside(setting) = asking.answered((item, Resolution::Replace)).unwrap() else {
        return;
    };
    let JobSettingAsideSetAsideOutcome::PlacingReplacement(replacing) = setting.set_aside((item, Some(AsideId(5)))).unwrap()
    else {
        return;
    };
    let JobPlacingReplacementPlacedOutcome::Restoring(restoring) = replacing.placed((item, Placed::Skip)).unwrap() else {
        return;
    };
    let _ = restoring.probed((ItemId(1), Found::Same));
}
