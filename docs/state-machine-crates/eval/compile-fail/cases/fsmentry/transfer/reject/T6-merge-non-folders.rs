// T6 merge-non-folders: merge a conflict that isn't folder onto folder.
// Such a conflict is `AskingFiles`; only `AskingFolders` has `merge`.
use impl_fsmentry::transfer::{JobEntry, Transfer};
use spec::transfer::{Config, Event, Found, ItemId, Machine};

fn main() {
    let i = ItemId(0);
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item: i, found: Found::Conflict { can_merge: false } }, &mut out);
    if let JobEntry::AskingFiles(asking) = m.entry() {
        asking.merge(i);
    }
}
