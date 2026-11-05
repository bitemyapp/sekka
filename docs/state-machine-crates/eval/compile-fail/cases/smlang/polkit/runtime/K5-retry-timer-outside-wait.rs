// K5 retry-timer-outside-wait. `RetryTimer` is declared only for
// `RetryWait`, and only its rows arm the timer; elsewhere the timer's event
// is `Err(InvalidEvent)` (refused at run time).
use impl_smlang::polkit::{Error, Events, Polkit, States};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    assert_eq!(sm.process_event(&mut out, Events::RetryTimer), Err(Error::InvalidEvent));
    sm.process_event(&mut out, Events::Begin((Cookie(7), 1, 0, true))).unwrap();
    assert_eq!(sm.process_event(&mut out, Events::RetryTimer), Err(Error::InvalidEvent));
    assert!(matches!(sm.state_enum(), States::Starting(_)));
    assert!(!out.iter().any(|e| matches!(e, Effect::ArmRetry { .. })));
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::StartHelper { .. })).count(), 1);
}
