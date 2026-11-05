// K1 answer-without-prompt. Driven through `consume`, the table has a
// `Submit` row only in `Prompted`; elsewhere `consume` returns `Err`
// (ignored by `handle`): refused at run time.
use impl_rust_fsm::polkit::Polkit;
use impl_rust_fsm::polkit::fsm::{Input, StateMachine};
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    use Input::*;
    for path in [&[][..], &[Begin][..], &[Begin, Prompt, Submit][..], &[Begin, Failure][..], &[Begin, FailureLast][..]] {
        let mut t = reach(path);
        let s = *t.state();
        assert_ne!(s, StateName::Prompted);
        assert!(t.consume(&Submit).is_err());
        assert_eq!(*t.state(), s);
    }
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    m.handle(Event::Submit, &mut out);
    assert_eq!(m.state(), StateName::Starting);
    assert!(!out.iter().any(|e| matches!(e, Effect::WriteAnswer { .. })));
}
