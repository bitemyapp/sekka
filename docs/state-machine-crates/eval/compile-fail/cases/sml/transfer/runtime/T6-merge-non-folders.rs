// T6 merge-non-folders. `Asking`'s payload keeps `can_merge` and the `merge`
// guard tests it: Merge on a file conflict takes the replace row and sets
// the target aside instead of listing. Refused at run time.
use impl_sml::transfer::{Reply, Transfer, TransferEvents as E, TransferStates};
use spec::transfer::{Config, Effect, Found, ItemId, Machine, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let item = ItemId(0);
    sm.process_event(&mut out, E::Start(1)).unwrap();
    sm.process_event(&mut out, E::Probed(Reply { item, value: Found::Conflict { can_merge: false } })).unwrap();
    out.clear();
    sm.process_event(&mut out, E::Answered(Reply { item, value: Resolution::Merge })).unwrap();
    assert_eq!(out, [Effect::SetAside { item }]);
    let mut setting_aside = false;
    sm.visit_current_states(|s| setting_aside = matches!(s, TransferStates::SettingAside(_)));
    assert!(setting_aside);
}
