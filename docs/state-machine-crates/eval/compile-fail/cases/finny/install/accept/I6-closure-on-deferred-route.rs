// I6 closure-on-deferred-route. `Prepared` keeps only the route, but
// `Preparing`'s `Cache` struct exists on every route: on the deferred one
// its closure size (never built: the placeholder 0) is read through
// `get_state`: accepted.
use impl_finny::install::{Cache, Install, Prepared, Preparing};
use spec::install::{Config, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB }, &mut out);
    assert_eq!(m.state(), StateName::Prepared);
    assert!(m.fsm().get_state::<Prepared>().deferred);
    let closure_mib = m.fsm().get_state::<Preparing>().get_state::<Cache>().closure_mib;
    assert_eq!(closure_mib, 0);
}
