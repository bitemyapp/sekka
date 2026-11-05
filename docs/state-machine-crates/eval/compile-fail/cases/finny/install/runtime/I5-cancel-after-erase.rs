// I5 cancel-after-erase. `Executing` has no `Cancel` row (only `Idle`,
// `Preparing` and `Prepared` do): during step 1 the cancel is ignored, the
// step's result goes on to step 2, and the session never ends `Cancelled`
// (refused at run time).
use impl_finny::install::Install;
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
    out.clear();
    m.handle(Event::Cancel, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Erase);
    m.handle(Event::StepDone, &mut out);
    assert_eq!(m.state(), StateName::Format);
    assert_eq!(out, vec![Effect::Progress(2), Effect::Run(Stage::Format)]);
    assert!(!out.contains(&Effect::Cancelled));
}
