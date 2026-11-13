// K1 answer-without-prompt. Only `prompted` handles `Submit`; elsewhere it
// bubbles to `active`, which ignores it (refused at run time).
use impl_statig::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Submit, &mut out);
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    assert_eq!(m.state(), StateName::Starting);
    m.handle(Event::Submit, &mut out);
    m.handle(Event::Unreachable { conv: 1 }, &mut out);
    assert_eq!(m.state(), StateName::RetryWait);
    m.handle(Event::Submit, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::WriteAnswer { .. })));
    assert_eq!(m.state(), StateName::RetryWait);
}
