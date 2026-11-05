// K2 stale-conversation. Every helper row is guarded by `current` (the
// event's conversation is the running one's); for an older one no guard
// passes and `process_event` returns `Err(TransitionsFailed)` (refused at
// run time).
use impl_smlang::polkit::{Error, Events, Polkit, States};
use spec::polkit::{Config, Cookie, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::Begin((Cookie(7), 2, 0, true))).unwrap();
    sm.process_event(&mut out, Events::ChooseIdentity(1)).unwrap();
    assert!(matches!(sm.state_enum(), States::Starting(run) if run.conv == 2));
    out.clear();
    assert_eq!(sm.process_event(&mut out, Events::Prompt((1, false))), Err(Error::TransitionsFailed));
    assert_eq!(sm.process_event(&mut out, Events::Success(1)), Err(Error::TransitionsFailed));
    assert!(out.is_empty());
    assert!(matches!(sm.state_enum(), States::Starting(_)));
}
