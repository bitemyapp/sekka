// I5 cancel-after-erase. `Cancel` has rows in `Idle`, `state<Preparing>` and
// `Prepared` only; under `state<Executing>` no row takes it (and the internal
// `Stop` can't be built outside the table), so the session can't end as
// `Cancelled` once erasing. Refused at run time.
use impl_sml::install::{Evaluated, Install, InstallEvents as E};
use spec::install::{Config, Effect, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let eval = Evaluated { download_mib: 2 * GIB, unpacked_mib: 10 * GIB, budget_mib: GIB };
    for event in [E::Begin, E::Checked(100 * GIB), E::Configured, E::Evaluated(eval), E::Confirm(true)] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    assert_eq!(m.state(), StateName::Erase);
    assert!(m.machine.process_event(&mut out, E::Cancel).is_err());
    assert_eq!(m.state(), StateName::Erase);
    m.machine.process_event(&mut out, E::StepDone).unwrap();
    assert!(m.machine.process_event(&mut out, E::Cancel).is_err());
    m.machine.process_event(&mut out, E::StageFailed).unwrap();
    assert_eq!(m.state(), StateName::Failed);
    assert!(out.contains(&Effect::Run(Stage::Erase)));
    assert!(!out.contains(&Effect::Cancelled));
    assert_eq!(out[out.len() - 2..], [Effect::AllowSleep, Effect::Failed { erased: true }]);
}
