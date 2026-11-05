// I4 deferred-without-space-check. On the deferred route `Installer`'s
// `Evaluated` rows out of `Preparing` are guarded on the root's room: the
// GEN-07 selection (300 GiB unpacked, a 39 GiB root) is refused, never
// prepared, and a confirmation erases nothing (refused at run time).
use impl_finny::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 39 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    out.clear();
    m.handle(Event::Evaluated { download_mib: 60 * GIB, unpacked_mib: 300 * GIB, budget_mib: 16 * GIB }, &mut out);
    assert_eq!(out, vec![Effect::Refused { needed_mib: 308 * GIB, root_mib: 39 * GIB }]);
    assert_eq!(m.state(), StateName::Refused);
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Prepared { .. } | Effect::Run(Stage::Erase))));
}
