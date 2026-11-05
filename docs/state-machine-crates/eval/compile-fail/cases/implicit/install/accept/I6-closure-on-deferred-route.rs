// I6 closure-on-deferred-route. `closure_bytes` is a pub field, readable on
// either route (0 on the deferred one, which never built a closure), and
// the deferred `Prepared` effect reports it: accepted.
use impl_implicit::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB }, &mut out);
    assert_eq!(m.state(), StateName::Prepared);
    assert!(m.deferred);
    let closure = m.closure_bytes;
    assert!(out.contains(&Effect::Prepared { deferred: true, closure_mib: closure }));
}
