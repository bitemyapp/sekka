// I3 confirm-during-preparation. Every preparation stage's `Confirm` row
// interrupts and fails; only `Prepared` has the row to `Erase` (refused at
// run time).
use impl_smlang::install::{Events, Install, States};
use spec::install::{Config, Effect, GIB, Machine, Stage};

fn main() {
    let mut m = Install::new(Config::default());
    let sm = &mut m;
    sm.process_event(Events::Begin).unwrap();
    sm.process_event(Events::Checked(100 * GIB)).unwrap();
    sm.process_event(Events::Confirm(true)).unwrap();
    assert!(matches!(sm.state_enum(), States::Failed));
    let pending = sm.context().pending();
    assert!(pending.ends_with(&[Effect::Interrupt, Effect::Failed { erased: false }]));
    assert!(!pending.contains(&Effect::Run(Stage::Erase)));
}
