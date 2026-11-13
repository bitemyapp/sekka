// K3 restart-without-backoff. A current failure in `Starting` or `Prompted`
// has only the back-off rows (to `RetryWait`, whose entry arms the timer, or
// `Unavailable`); only `Checking`'s failure targets `Starting`. Refused at
// run time.
use impl_sml::polkit::{Begin, Conv, Polkit, PolkitActiveStates, PolkitEvents as E};
use spec::polkit::{Config, Cookie, Effect, Machine, Note};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    sm.process_event(&mut out, E::Begin(begin)).unwrap();
    out.clear();
    sm.process_event(&mut out, E::Failure(Conv(1))).unwrap();
    assert_eq!(out, [Effect::EndHelper { conv: 1 }, Effect::ShowNote(Note::Retrying), Effect::ArmRetry { ms: 500 }]);
    assert!(sm.is(&PolkitActiveStates::RetryWait));
}
