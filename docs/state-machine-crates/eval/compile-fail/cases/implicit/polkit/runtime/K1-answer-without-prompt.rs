// K1 answer-without-prompt. `Submit` tests `busy` and `prompt` (SPEC-FIX):
// before the helper's first prompt nothing is written (refused at run time).
use impl_implicit::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    out.clear();
    m.handle(Event::Submit, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::WriteAnswer { .. })));
    assert_eq!(m.state(), StateName::Starting);
}
