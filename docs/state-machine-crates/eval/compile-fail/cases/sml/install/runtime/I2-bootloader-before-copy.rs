// I2 bootloader-before-copy. Only `Copy` and `BuildOnTarget` have a row to
// `Bootloader` (on `StepDone`): other events while copying find no row.
// Refused at run time.
use impl_sml::install::{Evaluated, Install, InstallEvents as E};
use spec::install::{Config, Effect, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let eval = Evaluated { download_mib: GIB, unpacked_mib: 4 * GIB, budget_mib: 2 * GIB };
    for event in [E::Begin, E::Checked(100 * GIB), E::Configured, E::Evaluated(eval), E::Built(4 * GIB), E::Cached] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    for event in [E::Confirm(true), E::StepDone, E::StepDone, E::StepDone] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    assert_eq!(m.state(), StateName::Copy);
    for event in [E::Cancel, E::Confirm(true), E::Cached, E::Begin] {
        assert!(m.machine.process_event(&mut out, event).is_err());
    }
    assert!(!out.contains(&Effect::Run(Stage::Bootloader)));
    m.machine.process_event(&mut out, E::StepDone).unwrap();
    let at = |e: Effect| out.iter().position(|x| *x == e).unwrap();
    assert!(at(Effect::Run(Stage::Copy)) < at(Effect::Run(Stage::Bootloader)));
}
