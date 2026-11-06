// I4 deferred-without-space-check: enter `Prepared` on the deferred route
// without the room check. `prepared` takes a `Route`, whose field is private:
// only the room checks inside `handle` build one.
use impl_fsmentry::install::{Install, Route, SessionEntry};
use spec::install::{Config, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 39 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    assert_eq!(m.state(), StateName::Evaluation);
    if let SessionEntry::Preparing(preparing) = m.entry() {
        preparing.prepared(Route { deferred: true });
    }
}
