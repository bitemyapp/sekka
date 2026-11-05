// T1 dispose-before-placed. The Trash or Delete is pushed only by the
// `Placed { Ok }` arm with an aside: an early `Disposed` is dropped (it
// needs `disposing`) and a failed placement restores the aside (refused at
// run time).
use impl_implicit::transfer::Transfer;
use spec::transfer::{AsideId, Config, Effect, Event, Found, ItemId, Machine, Placed, Resolution, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item, found: Found::Conflict { can_merge: false } }, &mut out);
    m.handle(Event::Answered { item, resolution: Resolution::Replace }, &mut out);
    m.handle(Event::SetAside { item, aside: Some(AsideId(5)) }, &mut out);
    m.handle(Event::Disposed { item, ok: true }, &mut out);
    assert_eq!(m.state(), StateName::PlacingReplacement);
    m.handle(Event::Placed { item, outcome: Placed::Skip }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Trash { .. } | Effect::Delete { .. })));
    assert_eq!(m.state(), StateName::Restoring);
}
