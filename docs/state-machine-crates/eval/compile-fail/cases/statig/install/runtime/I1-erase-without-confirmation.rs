// I1 erase-without-confirmation. `Run(Erase)` is `erase`'s entry action,
// and only `prepared` with a good `Confirm` enters it: step results and
// other events in a preparation stage or in `prepared` start nothing
// (refused at run time).
use impl_statig::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: GIB, unpacked_mib: 4 * GIB, budget_mib: 2 * GIB }, &mut out);
    m.handle(Event::Built { closure_mib: 4 * GIB }, &mut out);
    m.handle(Event::StepDone, &mut out);
    m.handle(Event::Cached, &mut out);
    assert_eq!(m.state(), StateName::Prepared);
    for event in [Event::StepDone, Event::Cached, Event::Begin, Event::StageFailed] {
        m.handle(event, &mut out);
    }
    assert!(!out.contains(&Effect::Run(Stage::Erase)));
    assert!(!out.contains(&Effect::StayAwake));
    assert_eq!(m.state(), StateName::Prepared);
}
