// T5 response-for-other-item. The typed transitions don't take an item: the
// runtime wrapper compares the event's item with the state's (`item()`) and
// drops another item's answer.
use impl_statum::transfer::Transfer;
use impl_statum::transfer::job::SomeState;
use spec::transfer::{Config, Event, Found, ItemId, Machine, Resolution, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 2 }, &mut out);
    m.handle(Event::Probed { item: ItemId(0), found: Found::Conflict { can_merge: false } }, &mut out);
    out.clear();
    m.handle(Event::Answered { item: ItemId(1), resolution: Resolution::Replace }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Asking);
    let SomeState::AskingFiles(s) = m.typed() else { panic!("not asking") };
    assert_eq!(s.state_data.item(), ItemId(0));
}
