// K4 double-reply. `Success` and the ending rows are `state<Active>`'s and
// all go to `Idle`, which takes no request event but `Begin`. Refused at run
// time.
use impl_sml::polkit::{Begin, Conv, Polkit, PolkitEvents as E, PolkitStates};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    sm.process_event(&mut out, E::Begin(begin)).unwrap();
    sm.process_event(&mut out, E::Success(Conv(1))).unwrap();
    assert!(sm.process_event(&mut out, E::Success(Conv(1))).is_err());
    assert!(sm.process_event(&mut out, E::Dismiss).is_err());
    assert!(sm.process_event(&mut out, E::CancelRequest(Cookie(7))).is_err());
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::Reply { .. })).count(), 1);
    assert!(sm.is(&PolkitStates::Idle));
}
