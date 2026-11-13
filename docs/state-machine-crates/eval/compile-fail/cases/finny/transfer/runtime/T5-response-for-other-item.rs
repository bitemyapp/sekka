// T5 response-for-other-item. Every guard compares the event's item with
// the state struct's: item 1's answer while item 0's sheet is up is
// `Err(NoTransition)` from the typed `dispatch` (refused at run time).
use impl_finny::finny::FsmError;
use impl_finny::transfer::{Answered, Asking, Transfer};
use spec::transfer::{Config, Event, Found, ItemId, Machine, Resolution, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 2 }, &mut out);
    m.handle(Event::Probed { item: ItemId(0), found: Found::Conflict { can_merge: false } }, &mut out);
    out.clear();
    let answer = Answered { item: ItemId(1), resolution: Resolution::Replace };
    assert_eq!(m.dispatch(answer, &mut out), Err(FsmError::NoTransition));
    m.handle(Event::Answered { item: ItemId(1), resolution: Resolution::Replace }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Asking);
    assert_eq!(m.fsm().get_state::<Asking>().item, ItemId(0));
}
