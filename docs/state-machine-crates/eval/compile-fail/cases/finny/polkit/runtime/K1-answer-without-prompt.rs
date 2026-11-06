// K1 answer-without-prompt. Only `Prompted` declares a `Submit`
// transition: the typed `dispatch` of `Submit` in `Starting` is
// `Err(NoTransition)`, and no answer is written (refused at run time).
use impl_finny::finny::FsmError;
use impl_finny::polkit::{ActiveEvents, Polkit, Submit};
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    assert_eq!(m.state(), StateName::Starting);
    assert_eq!(m.dispatch(ActiveEvents::from(Submit), &mut out), Err(FsmError::NoTransition));
    m.handle(Event::Submit, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::WriteAnswer { .. })));
    assert_eq!(m.state(), StateName::Starting);
}
