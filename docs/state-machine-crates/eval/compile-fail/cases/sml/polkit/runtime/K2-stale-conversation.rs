// K2 stale-conversation. Every helper event's guard compares its `conv` with
// the running conversation (`current`): conversation 1's events are dropped
// once 2 runs (`Err`, nothing emitted). Refused at run time.
use impl_sml::polkit::{Begin, Conv, Polkit, PolkitActiveStates, PolkitEvents as E, Prompt};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let begin = Begin { cookie: Cookie(7), identities: 2, preferred: 0, screen: true };
    sm.process_event(&mut out, E::Begin(begin)).unwrap();
    sm.process_event(&mut out, E::ChooseIdentity(1)).unwrap();
    assert!(out.contains(&Effect::StartHelper { conv: 2, identity: 1 }));
    out.clear();
    assert!(sm.process_event(&mut out, E::Prompt(Prompt { conv: 1, echo: false })).is_err());
    assert!(sm.process_event(&mut out, E::Success(Conv(1))).is_err());
    assert!(sm.process_event(&mut out, E::Failure(Conv(1))).is_err());
    assert!(out.is_empty());
    assert!(sm.is(&PolkitActiveStates::Starting));
}
