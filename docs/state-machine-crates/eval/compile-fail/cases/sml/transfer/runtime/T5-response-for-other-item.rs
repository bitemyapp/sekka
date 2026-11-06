// T5 response-for-other-item. Every guard compares the response's item with
// the state's payload: item 1's answer while item 0's sheet is up fails
// every guard (`Err`). Refused at run time.
use impl_sml::transfer::{Ask, Reply, Transfer, TransferEvents as E, TransferStates};
use spec::transfer::{Config, Effect, Found, ItemId, Machine, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    sm.process_event(&mut out, E::Start(2)).unwrap();
    let conflict = Found::Conflict { can_merge: false };
    sm.process_event(&mut out, E::Probed(Reply { item: ItemId(0), value: conflict })).unwrap();
    out.clear();
    let answer = Reply { item: ItemId(1), value: Resolution::Replace };
    assert!(sm.process_event(&mut out, E::Answered(answer)).is_err());
    assert!(out.is_empty());
    let mut asking = false;
    let expected = Ask { item: ItemId(0), can_merge: false };
    sm.visit_current_states(|s| asking = matches!(s, TransferStates::Asking(a) if *a == expected));
    assert!(asking);
}
