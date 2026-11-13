// T3 lost-aside. `Restoring` carries the aside; its only row is `Restored`
// for its item (reporting a stranded aside), so every other response, another
// item's result included, is taken by no row (`Err`). Refused at run time.
// The table could hold a row that drops it: nothing in the types forbids
// `Restoring(Restore) + Probed(..) / .. = Next`.
use impl_sml::transfer::{Aside, Reply, Restore, Transfer, TransferEvents as E, TransferStates};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, Machine, Placed, Problem, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let item = ItemId(0);
    sm.process_event(&mut out, E::Start(2)).unwrap();
    sm.process_event(&mut out, E::Probed(Reply { item, value: Found::Conflict { can_merge: false } })).unwrap();
    sm.process_event(&mut out, E::Answered(Reply { item, value: Resolution::Replace })).unwrap();
    sm.process_event(&mut out, E::SetAside(Reply { item, value: Some(AsideId(5)) })).unwrap();
    sm.process_event(&mut out, E::Placed(Reply { item, value: Placed::Skip })).unwrap();
    assert!(out.contains(&Effect::RestoreAside { item, aside: AsideId(5) }));
    let restoring = Restore { aside: Aside { item, aside: AsideId(5) }, cancel: false };
    for event in [
        E::Probed(Reply { item: ItemId(1), value: Found::Same }),
        E::Placed(Reply { item, value: Placed::Ok }),
        E::Disposed(Reply { item, value: true }),
        E::Restored(Reply { item: ItemId(1), value: true }),
    ] {
        assert!(sm.process_event(&mut out, event).is_err());
        let mut kept = false;
        sm.visit_current_states(|s| kept = matches!(s, TransferStates::Restoring(r) if *r == restoring));
        assert!(kept);
    }
    out.clear();
    sm.process_event(&mut out, E::Restored(Reply { item, value: false })).unwrap();
    assert_eq!(out[0], Effect::Problem { item, problem: Problem::Stranded(AsideId(5)) });
}
