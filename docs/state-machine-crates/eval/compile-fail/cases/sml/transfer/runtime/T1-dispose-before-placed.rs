// T1 dispose-before-placed. Trash or Delete is the action of
// `PlacingReplacement`'s `Placed { Ok }` row only: an early `Disposed` is
// taken by no row (`Err`), and a failed placement restores the aside.
// Refused at run time.
use impl_sml::transfer::{Reply, Transfer, TransferEvents as E, TransferStates};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, Machine, Placed, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let item = ItemId(0);
    sm.process_event(&mut out, E::Start(1)).unwrap();
    sm.process_event(&mut out, E::Probed(Reply { item, value: Found::Conflict { can_merge: false } })).unwrap();
    sm.process_event(&mut out, E::Answered(Reply { item, value: Resolution::Replace })).unwrap();
    assert!(sm.process_event(&mut out, E::Disposed(Reply { item, value: true })).is_err());
    sm.process_event(&mut out, E::SetAside(Reply { item, value: Some(AsideId(5)) })).unwrap();
    assert!(sm.process_event(&mut out, E::Disposed(Reply { item, value: true })).is_err());
    sm.process_event(&mut out, E::Placed(Reply { item, value: Placed::Skip })).unwrap();
    assert!(!out.iter().any(|e| matches!(e, Effect::Trash { .. } | Effect::Delete { .. })));
    let mut restoring = false;
    sm.visit_current_states(|s| restoring = matches!(s, TransferStates::Restoring(_)));
    assert!(restoring);
}
