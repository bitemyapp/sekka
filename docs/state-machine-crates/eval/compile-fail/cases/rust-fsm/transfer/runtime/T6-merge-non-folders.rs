// T6 merge-non-folders. `can_merge` is data beside the table, so the guard
// is in `handle`'s input function: a `Merge` answer to a conflict that
// can't merge becomes `Replace` (set aside), never `List`. Refused at run
// time; the bare table would take `Merge` in any `Asking`.
use impl_rust_fsm::transfer::Transfer;
use spec::transfer::{Config, Effect, Event, Found, ItemId, Machine, Resolution, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item, found: Found::Conflict { can_merge: false } }, &mut out);
    m.handle(Event::Answered { item, resolution: Resolution::Merge }, &mut out);
    assert_eq!(m.state(), StateName::SettingAside);
    assert!(!out.iter().any(|e| matches!(e, Effect::List { .. })));
}
