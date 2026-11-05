// I1 erase-without-confirmation. `Erase` is entered only by the `Prepared +
// Confirm [phrase_ok]` row (its entry hook runs the step); step results in
// a preparation stage or in `Prepared` are `Err(InvalidEvent)` (refused at
// run time).
use impl_smlang::install::{Error, Events, Install, States};
use spec::install::{Config, Effect, GIB, Machine, Stage};

fn main() {
    let mut m = Install::new(Config::default());
    let sm = &mut m;
    sm.process_event(Events::Begin).unwrap();
    sm.process_event(Events::Checked(100 * GIB)).unwrap();
    sm.process_event(Events::Configured).unwrap();
    sm.process_event(Events::Evaluated((GIB, 4 * GIB, 2 * GIB))).unwrap();
    assert_eq!(sm.process_event(Events::StepDone), Err(Error::InvalidEvent));
    sm.process_event(Events::Built(4 * GIB)).unwrap();
    sm.process_event(Events::Cached).unwrap();
    assert!(matches!(sm.state_enum(), States::Prepared(_)));
    for event in [Events::StepDone, Events::Cached, Events::Begin, Events::StageFailed] {
        assert_eq!(sm.process_event(event), Err(Error::InvalidEvent));
    }
    assert!(!sm.context().pending().contains(&Effect::Run(Stage::Erase)));
    assert!(matches!(sm.state_enum(), States::Prepared(_)));
}
