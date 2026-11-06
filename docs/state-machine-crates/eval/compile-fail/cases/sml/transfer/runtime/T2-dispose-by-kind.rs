// T2 dispose-by-kind. The `dispose` action reads `delete_replaced` alone: a
// `Restore` job without it trashes the replaced item (the kind is never
// read; the delete doesn't happen).
use impl_sml::transfer::{Reply, Transfer, TransferEvents as E};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, JobKind, Machine, Mode, Placed, Resolution};

fn main() {
    let mut m = Transfer::new(Config { mode: Mode::Move, kind: JobKind::Restore, delete_replaced: false });
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let item = ItemId(0);
    sm.process_event(&mut out, E::Start(1)).unwrap();
    sm.process_event(&mut out, E::Probed(Reply { item, value: Found::Conflict { can_merge: false } })).unwrap();
    sm.process_event(&mut out, E::Answered(Reply { item, value: Resolution::Replace })).unwrap();
    sm.process_event(&mut out, E::SetAside(Reply { item, value: Some(AsideId(5)) })).unwrap();
    sm.process_event(&mut out, E::Placed(Reply { item, value: Placed::Ok })).unwrap();
    assert!(out.contains(&Effect::Trash { item, aside: AsideId(5) }));
    assert!(!out.iter().any(|e| matches!(e, Effect::Delete { .. })));
}
