// I5 cancel-after-erase. `Cancel` is handled by `preparing` (and `idle`,
// `prepared`); under `executing` it is ignored, so the session can't end as
// `Cancelled` once erasing (refused at run time).
use impl_statig::install::Install;
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
    assert_eq!(m.state(), StateName::Erase);
    m.handle(Event::StepDone, &mut out);
    m.handle(Event::Cancel, &mut out);
    m.handle(Event::StageFailed, &mut out);
    assert_eq!(m.state(), StateName::Failed);
    assert!(out.contains(&Effect::Run(Stage::Erase)));
    assert!(!out.contains(&Effect::Cancelled));
    assert_eq!(out[out.len() - 2..], [Effect::AllowSleep, Effect::Failed { erased: true }]);
}
