// I2 bootloader-before-copy. `State::Files` leads to step 4 and only
// `Copy`/`BuildOnTarget` with `StepDone` lead to the bootloader: other
// events while copying change nothing (refused at run time).
use impl_enum::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 100 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: GIB, unpacked_mib: 4 * GIB, budget_mib: 2 * GIB }, &mut out);
    m.handle(Event::Built { closure_mib: 4 * GIB }, &mut out);
    m.handle(Event::Cached, &mut out);
    for event in [Event::Confirm { phrase_ok: true }, Event::StepDone, Event::StepDone, Event::StepDone] {
        m.handle(event, &mut out);
    }
    assert_eq!(m.state(), StateName::Copy);
    for event in [Event::Cancel, Event::Confirm { phrase_ok: true }, Event::Cached, Event::Begin] {
        m.handle(event, &mut out);
    }
    assert!(!out.contains(&Effect::Run(Stage::Bootloader)));
    m.handle(Event::StepDone, &mut out);
    let at = |e: Effect| out.iter().position(|x| *x == e).unwrap();
    assert!(at(Effect::Run(Stage::Copy)) < at(Effect::Run(Stage::Bootloader)));
}
