// I3 confirm-during-preparation. `state<Preparing>`'s `Confirm` row takes a
// confirmation from any stage: it interrupts and fails the session.
// Refused at run time.
use impl_sml::install::{Evaluated, Install, InstallEvents as E};
use spec::install::{Config, Effect, GIB, Machine, Stage, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    for event in [E::Begin, E::Checked(100 * GIB), E::Configured] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    out.clear();
    m.machine.process_event(&mut out, E::Confirm(true)).unwrap();
    assert_eq!(out, [Effect::Interrupt, Effect::Failed { erased: false }]);
    let eval = Evaluated { download_mib: GIB, unpacked_mib: 4 * GIB, budget_mib: 2 * GIB };
    assert!(m.machine.process_event(&mut out, E::Evaluated(eval)).is_err());
    assert!(!out.contains(&Effect::Run(Stage::Erase)));
    assert_eq!(m.state(), StateName::Failed);
}
