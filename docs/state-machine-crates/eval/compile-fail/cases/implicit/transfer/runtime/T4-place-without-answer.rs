// T4 place-without-answer. While `asked` is set the probe, set-aside and
// placement arms all return early: a stray probe result or placement
// doesn't place over the target (refused at run time).
use impl_implicit::transfer::Transfer;
use spec::transfer::{AsideId, Config, Effect, Event, Found, ItemId, Machine, Placed, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item, found: Found::Conflict { can_merge: false } }, &mut out);
    out.clear();
    m.handle(Event::Probed { item, found: Found::Free }, &mut out);
    m.handle(Event::SetAside { item, aside: Some(AsideId(5)) }, &mut out);
    m.handle(Event::Placed { item, outcome: Placed::Ok }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::PlaceAt { .. })));
    assert_eq!(m.state(), StateName::Asking);
}
