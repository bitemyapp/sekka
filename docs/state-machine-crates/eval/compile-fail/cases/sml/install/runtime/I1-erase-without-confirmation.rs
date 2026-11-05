// I1 erase-without-confirmation. `Run(Erase)` is `Erase`'s entry action, and
// only `Prepared`'s good-phrase `Confirm` row enters `state<Executing>`:
// step results and other events in a preparation stage or in `Prepared`
// start nothing. Refused at run time.
use impl_sml::install::{Evaluated, Install, InstallEvents as E};
use spec::install::{Config, Effect, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let eval = Evaluated { download_mib: GIB, unpacked_mib: 4 * GIB, budget_mib: 2 * GIB };
    for event in [E::Begin, E::Checked(100 * GIB), E::Configured, E::Evaluated(eval), E::Built(4 * GIB)] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    assert!(m.machine.process_event(&mut out, E::StepDone).is_err());
    m.machine.process_event(&mut out, E::Cached).unwrap();
    assert_eq!(m.state(), StateName::Prepared);
    for event in [E::StepDone, E::Cached, E::Begin, E::StageFailed] {
        assert!(m.machine.process_event(&mut out, event).is_err());
    }
    assert!(!out.contains(&Effect::Run(Stage::Erase)));
    assert!(!out.contains(&Effect::StayAwake));
    assert_eq!(m.state(), StateName::Prepared);
}
