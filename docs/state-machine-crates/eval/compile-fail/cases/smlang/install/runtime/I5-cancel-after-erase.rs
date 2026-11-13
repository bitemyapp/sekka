// I5 cancel-after-erase. `Cancel` is declared only for `Idle`, `Prepared`
// and the preparation stages; once erasing it is `Err(InvalidEvent)` and the
// steps go on (refused at run time).
use impl_smlang::install::{Error, Events, Install, States};
use spec::install::{Config, Effect, GIB, Machine};

fn main() {
    let mut m = Install::new(Config::default());
    m.process_event(Events::Begin).unwrap();
    m.process_event(Events::Checked(100 * GIB)).unwrap();
    m.process_event(Events::Configured).unwrap();
    m.process_event(Events::Evaluated((2 * GIB, 10 * GIB, GIB))).unwrap();
    m.process_event(Events::Confirm(true)).unwrap();
    assert!(matches!(m.state_enum(), States::Erase(_)));
    assert_eq!(m.process_event(Events::Cancel), Err(Error::InvalidEvent));
    m.process_event(Events::StepDone).unwrap();
    assert_eq!(m.process_event(Events::Cancel), Err(Error::InvalidEvent));
    assert!(matches!(m.state_enum(), States::Format(_)));
    assert!(!m.context().pending().contains(&Effect::Cancelled));
}
