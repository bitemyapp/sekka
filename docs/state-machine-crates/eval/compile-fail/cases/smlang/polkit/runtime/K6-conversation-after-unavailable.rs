// K6 conversation-after-unavailable. `Unavailable` declares only the
// request's rows and `ChooseIdentity`; the timer and helper events are
// `Err(InvalidEvent)` there (refused at run time).
use impl_smlang::polkit::{Error, Events, Polkit, States};
use spec::polkit::{Config, Cookie, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::Begin((Cookie(7), 1, 0, true))).unwrap();
    for conv in 1..=3 {
        sm.process_event(&mut out, Events::Unreachable(conv)).unwrap();
        if conv < 3 {
            sm.process_event(&mut out, Events::RetryTimer).unwrap();
        }
    }
    assert!(matches!(sm.state_enum(), States::Unavailable(_)));
    out.clear();
    assert_eq!(sm.process_event(&mut out, Events::RetryTimer), Err(Error::InvalidEvent));
    assert_eq!(sm.process_event(&mut out, Events::Unreachable(3)), Err(Error::InvalidEvent));
    assert!(out.is_empty());
}
