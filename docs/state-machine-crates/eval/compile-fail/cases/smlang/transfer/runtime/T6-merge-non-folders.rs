// T6 merge-non-folders. The `List` row is guarded by `merges` (Merge and
// `can_merge`); a Merge without it falls to the replace row (refused at
// run time).
use impl_smlang::transfer::{Events, States, Transfer};
use spec::transfer::{Config, Effect, Found, ItemId, Machine, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    let sm = &mut m;
    sm.process_event(&mut out, Events::Start(1)).unwrap();
    sm.process_event(&mut out, Events::Probed((item, Found::Conflict { can_merge: false }))).unwrap();
    sm.process_event(&mut out, Events::Answered((item, Resolution::Merge))).unwrap();
    assert!(!out.iter().any(|e| matches!(e, Effect::List { .. })));
    assert!(out.contains(&Effect::SetAside { item }));
    assert!(matches!(sm.state_enum(), States::SettingAside(_)));
}
