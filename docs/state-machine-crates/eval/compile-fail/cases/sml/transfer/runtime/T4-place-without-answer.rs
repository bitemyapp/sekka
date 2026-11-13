// T4 place-without-answer. `Asking`'s rows all take `Answered` for its item
// only: a stray probe result or placement is taken by no row (`Err`).
// Refused at run time.
use impl_sml::transfer::{Reply, Transfer, TransferEvents as E, TransferStates};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, Machine, Placed};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let item = ItemId(0);
    sm.process_event(&mut out, E::Start(1)).unwrap();
    sm.process_event(&mut out, E::Probed(Reply { item, value: Found::Conflict { can_merge: false } })).unwrap();
    out.clear();
    assert!(sm.process_event(&mut out, E::Probed(Reply { item, value: Found::Free })).is_err());
    assert!(sm.process_event(&mut out, E::SetAside(Reply { item, value: Some(AsideId(5)) })).is_err());
    assert!(sm.process_event(&mut out, E::Placed(Reply { item, value: Placed::Ok })).is_err());
    assert!(out.is_empty());
    let mut asking = false;
    sm.visit_current_states(|s| asking = matches!(s, TransferStates::Asking(_)));
    assert!(asking);
}
