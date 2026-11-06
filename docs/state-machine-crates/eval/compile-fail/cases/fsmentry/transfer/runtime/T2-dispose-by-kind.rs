// T2 dispose-by-kind. Trash or delete is the effect `handle` pushes on the
// `placed` edge (`PlacingReplacement -> Disposing`), from `delete_replaced`
// alone: a Put Back (`Restore`) job without it trashes (refused at run time).
use impl_fsmentry::transfer::Transfer;
use spec::transfer::{
    AsideId, Config, Effect, Event, Found, ItemId, JobKind, Machine, Mode, Placed, Resolution,
};

fn main() {
    let i = ItemId(0);
    let a = AsideId(3);
    let mut m = Transfer::new(Config { mode: Mode::Move, kind: JobKind::Restore, delete_replaced: false });
    let mut out = Vec::new();
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item: i, found: Found::Conflict { can_merge: false } }, &mut out);
    m.handle(Event::Answered { item: i, resolution: Resolution::Replace }, &mut out);
    m.handle(Event::SetAside { item: i, aside: Some(a) }, &mut out);
    out.clear();
    m.handle(Event::Placed { item: i, outcome: Placed::Ok }, &mut out);
    assert_eq!(out, vec![Effect::Trash { item: i, aside: a }]);
}
