// T3 lost-aside: move on to the next item from `Disposing` before the
// disposal's result. `Disposing`'s only ways out are `disposed_next` and
// `disposed_finish` (and `Restoring`'s `restored_next`/`restored_finish`);
// the shared `next` isn't one of them. (Each hands the aside back, and
// nothing makes the caller use it: the methods aren't `#[must_use]`.)
use impl_fsmentry::transfer::{JobEntry, Transfer};
use spec::transfer::{AsideId, Config, Event, Found, ItemId, Machine, Placed, Resolution, StateName};

fn main() {
    let i = ItemId(0);
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 2 }, &mut out);
    m.handle(Event::Probed { item: i, found: Found::Conflict { can_merge: false } }, &mut out);
    m.handle(Event::Answered { item: i, resolution: Resolution::Replace }, &mut out);
    m.handle(Event::SetAside { item: i, aside: Some(AsideId(3)) }, &mut out);
    m.handle(Event::Placed { item: i, outcome: Placed::Ok }, &mut out);
    assert_eq!(m.state(), StateName::Disposing);
    if let JobEntry::Disposing(disposing) = m.entry() {
        disposing.next(ItemId(1));
    }
}
