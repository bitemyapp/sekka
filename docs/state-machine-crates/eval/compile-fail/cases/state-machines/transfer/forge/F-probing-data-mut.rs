// Forge (T5 response-for-other-item). The generated, public
// `probing_data_mut` rewrites the item being probed: item 1's probe result
// then counts while item 0's was awaited. Other means that stay public:
// `with_<state>_data`, `<state>_data_mut` and the generic
// `state_data_<state>_mut` on every typed machine (`running_data_mut` can
// replace the frame stack with `Frames::default()`); `set_<state>_data` and
// `<state>_data_mut` on the wrapper `into_dynamic()` returns;
// `initialize()` on every state type (transfer has no entry hooks).
use impl_state_machines::transfer::{Ctx, Job, JobIdleStartOutcome, JobProbingProbedOutcome};
use spec::transfer::{Config, Effect, Found, ItemId, Name};

fn main() {
    let JobIdleStartOutcome::Probing(mut probing) = Job::new(Ctx::new(Config::default())).start(2).unwrap() else {
        panic!()
    };
    *probing.probing_data_mut().unwrap() = ItemId(1);
    probing.take_effects();
    let JobProbingProbedOutcome::Placing(mut placing) = probing.probed((ItemId(1), Found::Free)).unwrap() else { panic!() };
    assert_eq!(placing.take_effects(), vec![Effect::PlaceAt { item: ItemId(1), name: Name::Target }]);
}
