// T5 response-for-other-item. Each arm compares the event's item with
// `current`: item 1's answer while item 0's sheet is up is dropped (refused
// at run time).
use impl_implicit::transfer::Transfer;
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
    assert_eq!(m.current, Some(ItemId(0)));
}
