// T6 merge-non-folders. `asking` keeps `can_merge` and its handler tests it:
// Merge on a file conflict sets the target aside instead of listing
// (refused at run time).
use impl_statig::transfer::Transfer;
use spec::transfer::{Config, Effect, Event, Found, ItemId, Machine, Resolution, StateName};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item, found: Found::Conflict { can_merge: false } }, &mut out);
    out.clear();
    m.handle(Event::Answered { item, resolution: Resolution::Merge }, &mut out);
    assert_eq!(out, vec![Effect::SetAside { item }]);
    assert_eq!(m.state(), StateName::SettingAside);
}
