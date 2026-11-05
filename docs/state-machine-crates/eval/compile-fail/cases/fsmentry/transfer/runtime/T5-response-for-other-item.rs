// T5 response-for-other-item. The item an event names is its data, which no
// edge takes: every arm of `handle` guards on the awaited item
// (`*s.as_ref() == item`), and item 1's answer is dropped while item 0's is
// awaited (refused at run time).
use impl_fsmentry::transfer::Transfer;
use spec::transfer::{Config, Event, Found, ItemId, Machine, Resolution, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 2 }, &mut out);
    m.handle(Event::Probed { item: ItemId(0), found: Found::Conflict { can_merge: true } }, &mut out);
    out.clear();
    m.handle(Event::Answered { item: ItemId(1), resolution: Resolution::Replace }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Asking);
}
