// T5 response-for-other-item. Every response event has the event-level
// `current` guard: the typed `probed` for item 1 while probing item 0 is
// refused and the machine handed back (refused at run time;
// `forge/F-probing-data-mut.rs` defeats it by rewriting the item).
use impl_state_machines::transfer::{Ctx, Job, JobIdleStartOutcome, JobProbingProbedOutcome};
use spec::transfer::{Config, Effect, Found, ItemId, Name};

fn main() {
    let JobIdleStartOutcome::Probing(mut probing) = Job::new(Ctx::new(Config::default())).start(2).unwrap() else {
        panic!()
    };
    probing.take_effects();
    let Err((mut probing, _)) = probing.probed((ItemId(1), Found::Free)) else { panic!("item 1's probe was taken") };
    assert!(probing.take_effects().is_empty());
    let JobProbingProbedOutcome::Placing(mut placing) = probing.probed((ItemId(0), Found::Free)).unwrap() else { panic!() };
    assert_eq!(placing.take_effects(), vec![Effect::PlaceAt { item: ItemId(0), name: Name::Target }]);
}
