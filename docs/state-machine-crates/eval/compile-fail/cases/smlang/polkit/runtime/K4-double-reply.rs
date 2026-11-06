// K4 double-reply. `Idle` carries no request, and every event that replies
// for one is undeclared there: after `Success` the second `Success` and a
// `Dismiss` are `Err(InvalidEvent)` (refused at run time).
use impl_smlang::polkit::{Error, Events, Polkit, States};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::Begin((Cookie(7), 1, 0, true))).unwrap();
    sm.process_event(&mut out, Events::Success(1)).unwrap();
    assert!(matches!(sm.state_enum(), States::Idle));
    assert_eq!(sm.process_event(&mut out, Events::Success(1)), Err(Error::InvalidEvent));
    assert_eq!(sm.process_event(&mut out, Events::Dismiss), Err(Error::InvalidEvent));
    assert_eq!(sm.process_event(&mut out, Events::CancelRequest(Cookie(7))), Err(Error::InvalidEvent));
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::Reply { .. })).count(), 1);
}
