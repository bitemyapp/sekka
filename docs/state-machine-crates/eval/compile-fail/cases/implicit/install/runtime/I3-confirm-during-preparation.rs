// I3 confirm-during-preparation. SPEC-FIX: a `Confirm` while preparing
// interrupts and fails the session (the helper queued it and executed the
// moment preparation ended): refused at run time.
use impl_implicit::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    out.clear();
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    assert_eq!(out, vec![Effect::Interrupt, Effect::Failed { erased: false }]);
    m.handle(Event::Evaluated { download_mib: GIB, unpacked_mib: 4 * GIB, budget_mib: 2 * GIB }, &mut out);
    assert!(!out.contains(&Effect::Run(Stage::Erase)));
    assert_eq!(m.state(), StateName::Failed);
}
