// T2 dispose-by-kind. The `Dispose` action reads only `delete_replaced`:
// a `Restore` job without it trashes the replaced item (refused at run
// time; the table has no say in it).
use impl_rust_fsm::transfer::Transfer;
use spec::transfer::{AsideId, Config, Effect, Event, Found, ItemId, JobKind, Machine, Mode, Placed, Resolution};

fn main() {
    let mut m = Transfer::new(Config { mode: Mode::Move, kind: JobKind::Restore, delete_replaced: false });
    let mut out = Vec::new();
    let item = ItemId(0);
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item, found: Found::Conflict { can_merge: false } }, &mut out);
    m.handle(Event::Answered { item, resolution: Resolution::Replace }, &mut out);
    m.handle(Event::SetAside { item, aside: Some(AsideId(3)) }, &mut out);
    m.handle(Event::Placed { item, outcome: Placed::Ok }, &mut out);
    assert!(out.contains(&Effect::Trash { item, aside: AsideId(3) }));
    assert!(!out.iter().any(|e| matches!(e, Effect::Delete { .. })));
}
