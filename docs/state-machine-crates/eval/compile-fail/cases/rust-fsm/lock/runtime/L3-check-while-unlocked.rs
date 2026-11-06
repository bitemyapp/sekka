// L3 check-while-unlocked. Driven through `consume`, the `screen` table has
// a `Submit` row only in `Locked` (Locked.Idle); elsewhere `consume`
// returns `Err`: refused at run time.
use impl_rust_fsm::lock::screen::{Input, StateMachine};
use spec::lock::{Config, Effect, Event, LockPhase, Machine};

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    use Input::*;
    for path in [&[][..], &[Lock][..], &[Lock, LockConfirmed, Submit][..]] {
        let mut t = reach(path);
        assert_ne!(*t.state(), LockPhase::Locked);
        assert!(t.consume(&Submit).is_err());
    }
    let mut m = impl_rust_fsm::lock::Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Lock, &mut out);
    m.handle(Event::Typed { empty: false }, &mut out);
    m.handle(Event::Submit, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::StartCheck { .. })));
}
