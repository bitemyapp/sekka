// T5 response-for-other-item. Every handler guards on its state-local item:
// item 1's answer while item 0's sheet is up is dropped (refused at run time).
use impl_statig::transfer::{State, Transfer};
use spec::transfer::{Config, Event, Found, ItemId, Machine, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 2 }, &mut out);
    m.handle(Event::Probed { item: ItemId(0), found: Found::Conflict { can_merge: false } }, &mut out);
    out.clear();
    m.handle(Event::Answered { item: ItemId(1), resolution: Resolution::Replace }, &mut out);
    assert!(out.is_empty());
    assert_eq!(*m.state_enum(), State::Asking { item: ItemId(0), can_merge: false });
}
