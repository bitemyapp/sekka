// T5 response-for-other-item. The table has no items (its inputs are bare
// response kinds); `handle`'s input function drops a response that names
// another item before it reaches the table: refused at run time.
use impl_rust_fsm::transfer::Transfer;
use spec::transfer::{Config, Event, Found, ItemId, Machine, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 2 }, &mut out);
    out.clear();
    m.handle(Event::Probed { item: ItemId(1), found: Found::Free }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Probing);
}
