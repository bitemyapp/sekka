// K6 conversation-after-unavailable. `Unavailable`'s only row is
// `ChooseIdentity`; helper events find no running conversation (`current`
// fails) and `RetryTimer` has no row there. Refused at run time.
use impl_sml::polkit::{Begin, Conv, Polkit, PolkitActiveStates, PolkitEvents as E};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config { retry_base_ms: 500, max_unanswered: 1 });
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    sm.process_event(&mut out, E::Begin(begin)).unwrap();
    sm.process_event(&mut out, E::Unreachable(Conv(1))).unwrap();
    assert!(sm.is(&PolkitActiveStates::Unavailable));
    out.clear();
    for event in [E::RetryTimer, E::Failure(Conv(1)), E::Unreachable(Conv(1)), E::Submit] {
        assert!(sm.process_event(&mut out, event).is_err());
    }
    assert!(out.is_empty());
    sm.process_event(&mut out, E::ChooseIdentity(0)).unwrap();
    assert_eq!(out, [Effect::StartHelper { conv: 2, identity: 0 }]);
}
