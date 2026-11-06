// I5 cancel-after-erase. `Cancel` latches only while a preparation stage
// runs: once erasing it is ignored and the session goes on to step 2
// (refused at run time).
use impl_implicit::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB }, &mut out);
    m.handle(Event::Confirm { phrase_ok: true }, &mut out);
    assert_eq!(m.state(), StateName::Erase);
    m.handle(Event::Cancel, &mut out);
    assert!(!m.cancel);
    m.handle(Event::StepDone, &mut out);
    assert_eq!(m.state(), StateName::Format);
    assert!(out.contains(&Effect::Run(Stage::Format)));
    assert!(!out.contains(&Effect::Cancelled));
}
