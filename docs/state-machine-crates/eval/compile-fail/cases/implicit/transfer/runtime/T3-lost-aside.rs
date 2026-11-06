// T3 lost-aside. While `restoring` is set, the only event the flags let
// through is `Restored` for the current item: the next item's probe result
// and a stray disposal are dropped (refused at run time).
use impl_implicit::transfer::Transfer;
use spec::transfer::{AsideId, Config, Event, Found, ItemId, Machine, Placed, Resolution, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    m.handle(Event::Start { items: 2 }, &mut out);
    m.handle(Event::Probed { item, found: Found::Conflict { can_merge: false } }, &mut out);
    m.handle(Event::Answered { item, resolution: Resolution::Replace }, &mut out);
    m.handle(Event::SetAside { item, aside: Some(AsideId(5)) }, &mut out);
    m.handle(Event::Placed { item, outcome: Placed::Skip }, &mut out);
    assert_eq!(m.state(), StateName::Restoring);
    out.clear();
    m.handle(Event::Probed { item: ItemId(1), found: Found::Same }, &mut out);
    m.handle(Event::Disposed { item, ok: true }, &mut out);
    m.handle(Event::Answered { item, resolution: Resolution::Skip }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.aside, Some(AsideId(5)));
    assert_eq!(m.state(), StateName::Restoring);
}
