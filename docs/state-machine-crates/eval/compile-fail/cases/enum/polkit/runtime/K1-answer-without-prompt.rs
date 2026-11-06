// K1 answer-without-prompt. impl-enum's `handle` writes an answer only for
// `Phase::Prompted`; a `Submit` while starting falls through (refused at
// run time).
use impl_enum::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    assert_eq!(m.state(), StateName::Starting);
    m.handle(Event::Submit, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::WriteAnswer { .. })));
    assert_eq!(m.state(), StateName::Starting);
}
