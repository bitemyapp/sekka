// T3 lost-aside. `State::Restoring` holds the aside, and `handle` leaves it
// only on `Restored` for its item: the next item's probe result is dropped,
// so the job can't go on with the aside unrestored (refused at run time).
use impl_enum::transfer::{State, Transfer};
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
    assert!(out.is_empty());
    assert!(matches!(m.typed(), State::Restoring { aside: AsideId(5), .. }));
}
