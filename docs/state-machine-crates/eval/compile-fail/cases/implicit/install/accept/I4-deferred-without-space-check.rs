// I4 deferred-without-space-check. BUG(GEN-07): the root-space guard runs
// only after an in-memory build; 300 GiB unpacked on a 39 GiB root is
// `Prepared` on the deferred route: accepted.
use impl_implicit::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 39 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 60 * GIB, unpacked_mib: 300 * GIB, budget_mib: 16 * GIB }, &mut out);
    assert_eq!(m.state(), StateName::Prepared);
    assert!(out.contains(&Effect::Prepared { deferred: true, closure_mib: 0 }));
    assert!(!out.iter().any(|e| matches!(e, Effect::Refused { .. })));
}
