// K1 answer-without-prompt. Only `Prompted` has a `Submit` row; elsewhere no
// row takes it (`Err`). Refused at run time.
use impl_sml::polkit::{Begin, Conv, Polkit, PolkitActiveStates, PolkitEvents as E};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    assert!(sm.process_event(&mut out, E::Submit).is_err());
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    sm.process_event(&mut out, E::Begin(begin)).unwrap();
    assert!(sm.process_event(&mut out, E::Submit).is_err());
    sm.process_event(&mut out, E::Unreachable(Conv(1))).unwrap();
    assert!(sm.is(&PolkitActiveStates::RetryWait));
    assert!(sm.process_event(&mut out, E::Submit).is_err());
    assert!(!out.iter().any(|e| matches!(e, Effect::WriteAnswer { .. })));
}
