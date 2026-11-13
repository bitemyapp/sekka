// T2 dispose-by-kind. The `dispose` action reads only `delete_replaced`: a
// `Restore` job without it trashes (refused at run time; the context keeps
// the whole `Config`, so nothing in the types keeps `kind` out).
use impl_smlang::transfer::{Events, Transfer};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, JobKind, Machine, Mode, Placed, Resolution};

fn main() {
    let config = Config { mode: Mode::Move, kind: JobKind::Restore, delete_replaced: false };
    let mut m = Transfer::new(config);
    let mut out = Vec::new();
    let item = ItemId(0);
    let sm = &mut m;
    sm.process_event(&mut out, Events::Start(1)).unwrap();
    sm.process_event(&mut out, Events::Probed((item, Found::Conflict { can_merge: false }))).unwrap();
    sm.process_event(&mut out, Events::Answered((item, Resolution::Replace))).unwrap();
    sm.process_event(&mut out, Events::SetAside((item, Some(AsideId(5))))).unwrap();
    sm.process_event(&mut out, Events::Placed((item, Placed::Ok))).unwrap();
    assert!(out.contains(&Effect::Trash { item, aside: AsideId(5) }));
    assert!(!out.iter().any(|e| matches!(e, Effect::Delete { .. })));
}
