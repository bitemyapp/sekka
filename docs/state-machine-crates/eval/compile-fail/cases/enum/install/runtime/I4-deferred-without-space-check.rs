// I4 deferred-without-space-check. `handle` checks the root's room before
// building `State::Prepared` on the deferred route: the selection that
// doesn't fit is refused, and a confirmation then erases nothing (refused
// at run time).
use impl_enum::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 39 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 60 * GIB, unpacked_mib: 300 * GIB, budget_mib: 29 * GIB }, &mut out);
    assert_eq!(m.state(), StateName::Refused);
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    assert!(out.iter().any(|e| matches!(e, Effect::Refused { .. })));
    assert!(!out.contains(&Effect::Run(Stage::Erase)));
}
