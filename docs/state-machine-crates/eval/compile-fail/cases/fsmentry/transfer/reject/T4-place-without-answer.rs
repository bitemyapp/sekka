// T4 place-without-answer: place over the conflicting target while the
// sheet is up. Every way out of `AskingFiles` is an answer (`next`/`finish`
// for Skip and Cancel, `keep_both`, `replace`); placing at the target is
// `Probing`'s `free`.
use impl_fsmentry::transfer::{JobEntry, Transfer};
use spec::transfer::{Config, Event, Found, ItemId, Machine};

fn main() {
    let i = ItemId(0);
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Start { items: 1 }, &mut out);
    m.handle(Event::Probed { item: i, found: Found::Conflict { can_merge: false } }, &mut out);
    if let JobEntry::AskingFiles(asking) = m.entry() {
        asking.free(i);
    }
}
