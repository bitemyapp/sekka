// K5 retry-timer-outside-wait. Only `RetryWait` has a `RetryTimer` row, and
// only its entry action arms the timer. Refused at run time.
use impl_sml::polkit::{Begin, Conv, Polkit, PolkitActiveStates, PolkitEvents as E, Prompt};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    assert!(sm.process_event(&mut out, E::RetryTimer).is_err());
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    sm.process_event(&mut out, E::Begin(begin)).unwrap();
    out.clear();
    assert!(sm.process_event(&mut out, E::RetryTimer).is_err());
    assert!(out.is_empty());
    sm.process_event(&mut out, E::Prompt(Prompt { conv: 1, echo: false })).unwrap();
    sm.process_event(&mut out, E::Failure(Conv(1))).unwrap();
    assert!(sm.is(&PolkitActiveStates::RetryWait));
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::ArmRetry { .. })).count(), 1);
}
