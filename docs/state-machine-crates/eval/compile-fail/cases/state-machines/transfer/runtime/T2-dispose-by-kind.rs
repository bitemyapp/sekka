// T2 dispose-by-kind. A `Restore` job without `delete_replaced`: the typed
// `placed` Ok from `PlacingReplacement` runs `dispose`, which reads only
// `delete_replaced` and trashes the replaced item (HAKO-05; refused at run
// time: the callback decides).
use impl_state_machines::transfer::{
    Ctx, Job, JobAskingUnmergeableAnsweredOutcome, JobIdleStartOutcome, JobPlacingReplacementPlacedOutcome, JobProbingProbedOutcome,
    JobSettingAsideSetAsideOutcome,
};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, JobKind, Mode, Placed, Resolution};

fn main() {
    let config = Config { mode: Mode::Move, kind: JobKind::Restore, delete_replaced: false };
    let item = ItemId(0);
    let JobIdleStartOutcome::Probing(probing) = Job::new(Ctx::new(config)).start(1).unwrap() else { panic!() };
    let conflict = Found::Conflict { can_merge: false };
    let JobProbingProbedOutcome::AskingUnmergeable(asking) = probing.probed((item, conflict)).unwrap() else { panic!() };
    let JobAskingUnmergeableAnsweredOutcome::SettingAside(setting) = asking.answered((item, Resolution::Replace)).unwrap() else {
        panic!()
    };
    let JobSettingAsideSetAsideOutcome::PlacingReplacement(mut replacing) = setting.set_aside((item, Some(AsideId(5)))).unwrap()
    else {
        panic!()
    };
    replacing.take_effects();
    let JobPlacingReplacementPlacedOutcome::Disposing(mut disposing) = replacing.placed((item, Placed::Ok)).unwrap() else {
        panic!()
    };
    assert_eq!(disposing.take_effects(), vec![Effect::Trash { item, aside: AsideId(5) }]);
}
