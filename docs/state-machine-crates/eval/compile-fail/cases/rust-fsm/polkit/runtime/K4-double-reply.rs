// K4 double-reply. Every row that replies leaves `Active` for `Idle`, and
// `Idle` has no row that replies to the ended request (a repeated cancel
// is `Err`; another cookie's cancel never reaches the table): refused at
// run time.
use impl_rust_fsm::polkit::Polkit;
use impl_rust_fsm::polkit::fsm::{Input, StateMachine};
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut t = StateMachine::new();
    t.consume(&Input::Begin).expect("a row");
    t.consume(&Input::Dismiss).expect("a row");
    assert_eq!(*t.state(), StateName::Idle);
    assert!(t.consume(&Input::Dismiss).is_err());

    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    m.handle(Event::Success { conv: 1 }, &mut out);
    m.handle(Event::CancelRequest { cookie: Cookie(7) }, &mut out);
    m.handle(Event::Dismiss, &mut out);
    m.handle(Event::AuthorityChanged, &mut out);
    let replies = out.iter().filter(|e| matches!(e, Effect::Reply { cookie: Cookie(7), .. })).count();
    assert_eq!(replies, 1);
}
