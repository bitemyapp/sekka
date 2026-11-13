// I2 bootloader-before-copy. `Bootloader` is entered only from `Copy` and
// `BuildOnTarget`; `Files + StepDone` goes to step 4, and no other event
// moves `Files` on (refused at run time).
use impl_smlang::install::{Error, Events, Install, States};
use spec::install::{Config, Effect, GIB, Machine, Stage};

fn main() {
    let mut m = Install::new(Config::default());
    let sm = &mut m;
    sm.process_event(Events::Begin).unwrap();
    sm.process_event(Events::Checked(100 * GIB)).unwrap();
    sm.process_event(Events::Configured).unwrap();
    sm.process_event(Events::Evaluated((2 * GIB, 10 * GIB, GIB))).unwrap();
    sm.process_event(Events::Confirm(true)).unwrap();
    sm.process_event(Events::StepDone).unwrap();
    sm.process_event(Events::StepDone).unwrap();
    assert!(matches!(sm.state_enum(), States::Files(_)));
    assert_eq!(sm.process_event(Events::Cached), Err(Error::InvalidEvent));
    sm.process_event(Events::StepDone).unwrap();
    assert!(matches!(sm.state_enum(), States::BuildOnTarget));
    let pending = sm.context().pending();
    assert!(pending.ends_with(&[Effect::Progress(4), Effect::Run(Stage::BuildOnTarget)]));
    assert!(!pending.contains(&Effect::Run(Stage::Bootloader)));
}
