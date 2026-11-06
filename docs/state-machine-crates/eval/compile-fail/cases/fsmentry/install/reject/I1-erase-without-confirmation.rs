// I1 erase-without-confirmation: start the destructive steps from
// `Prepared` without the confirmation. `Prepared`'s only edge to
// `Executing` is `confirm`, and it takes `Steps`, which only
// `Steps::confirmed(route, phrase_ok)` builds (and only for a matching
// phrase).
use impl_fsmentry::install::{Exec, Install, SessionEntry, Steps};
use spec::install::{Config, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB }, &mut out);
    assert_eq!(m.state(), StateName::Prepared);
    if let SessionEntry::Prepared(prepared) = m.entry() {
        prepared.confirm(Steps(Exec::Erase(true)));
    }
}
