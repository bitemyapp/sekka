// T3 lost-aside. `Restoring` is left only by `Restored` for its item (its
// rows' guards): other responses, and another item's, change nothing; the
// restore's failure is reported (`Stranded`) before the item's outcome and
// the next probe (refused at run time).
use impl_finny::transfer::Transfer;
use spec::transfer::{AsideId, Config, Effect, Event, Found, ItemId, Machine, Outcome, Placed, Problem, Resolution, StateName};

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
    let other = ItemId(1);
    for event in [
        Event::Probed { item: other, found: Found::Same },
        Event::Placed { item: other, outcome: Placed::Ok },
        Event::Disposed { item: other, ok: true },
        Event::Restored { item: other, ok: true },
        Event::Disposed { item, ok: true },
        Event::Placed { item, outcome: Placed::Ok },
    ] {
        m.handle(event, &mut out);
    }
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Restoring);
    m.handle(Event::Restored { item, ok: false }, &mut out);
    assert_eq!(
        out,
        vec![
            Effect::Problem { item, problem: Problem::Stranded(AsideId(5)) },
            Effect::Outcome { item, outcome: Outcome::Failed },
            Effect::Probe { item: other },
        ]
    );
}
