// I6 closure-on-deferred-route. `closure_mib` is a field beside the table,
// present on both routes: on the deferred route (no build, no closure)
// `data().closure_mib` is readable in `Prepared` and after: accepted.
use impl_rust_fsm::install::Install;
use spec::install::{Config, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 400 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 60 * GIB, unpacked_mib: 300 * GIB, budget_mib: 16 * GIB }, &mut out);
    assert_eq!(m.state(), StateName::Prepared);
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    assert_eq!(m.state(), StateName::Erase);
    let closure: u64 = m.data().closure_mib;
    assert_eq!(closure, 0);
}
