// F-as-mut (transfer): commits T5 response-for-other-item. fsmentry
// generates `AsMut<ItemId>` on the `Probing` handle (and on every other data
// state's), public with the handles: rewriting the awaited item to 1 makes
// item 1's `Probed` count while item 0's was awaited. (Other crate-generated
// means: `AsMut<Replaced>`/`AsMut<Restore>`, whose fields are private, for
// swapping asides between two live machines; every exit handing back its
// data for another machine's edge to take.)
use impl_fsmentry::transfer::{JobEntry, Transfer};
use spec::transfer::{Config, Effect, Event, Found, ItemId, Machine, Name};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 2 }, &mut out);
    if let JobEntry::Probing(mut probing) = m.entry() {
        *probing.as_mut() = ItemId(1);
    }
    out.clear();
    m.handle(Event::Probed { item: ItemId(1), found: Found::Free }, &mut out);
    assert_eq!(out, vec![Effect::PlaceAt { item: ItemId(1), name: Name::Target }]);
}
