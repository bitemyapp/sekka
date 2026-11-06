// T3 lost-aside. `restoring` carries the aside, and its handler leaves only
// on `Restored` for its item (reporting a stranded aside); everything else,
// another item's result included, is ignored (refused at run time). Any
// handler could return a transition that drops it: nothing in the types
// forbids `Transition(State::probing(..))` here.
use impl_statig::transfer::{After, State, Transfer};
use spec::transfer::{AsideId, Config, Effect, Event, Found, ItemId, Machine, Placed, Problem, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    m.handle(Event::Start { items: 2 }, &mut out);
    m.handle(Event::Probed { item, found: Found::Conflict { can_merge: false } }, &mut out);
    m.handle(Event::Answered { item, resolution: Resolution::Replace }, &mut out);
    m.handle(Event::SetAside { item, aside: Some(AsideId(5)) }, &mut out);
    m.handle(Event::Placed { item, outcome: Placed::Skip }, &mut out);
    assert!(out.contains(&Effect::RestoreAside { item, aside: AsideId(5) }));
    let restoring = State::Restoring { item, aside: AsideId(5), then: After::Failed };
    for event in [
        Event::Probed { item: ItemId(1), found: Found::Same },
        Event::Placed { item, outcome: Placed::Ok },
        Event::Disposed { item, ok: true },
        Event::Restored { item: ItemId(1), ok: true },
    ] {
        m.handle(event, &mut out);
        assert_eq!(*m.state_enum(), restoring);
    }
    out.clear();
    m.handle(Event::Restored { item, ok: false }, &mut out);
    assert_eq!(out[0], Effect::Problem { item, problem: Problem::Stranded(AsideId(5)) });
}
