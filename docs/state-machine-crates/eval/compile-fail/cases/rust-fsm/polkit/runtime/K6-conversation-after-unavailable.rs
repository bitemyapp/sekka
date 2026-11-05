// K6 conversation-after-unavailable. Driven through `consume`,
// `Unavailable` leaves for `Starting` only on `ChooseIdentity` (or a new
// request); its timer and helper rows don't exist: refused at run time.
use impl_rust_fsm::polkit::fsm::{Input, StateMachine};
use spec::polkit::StateName;

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    use Input::*;
    for i in [RetryTimer, Failure, Unreachable, Prompt, Submit] {
        let mut t = reach(&[Begin, FailureLast]);
        assert_eq!(*t.state(), StateName::Unavailable);
        assert!(t.consume(&i).is_err());
        assert_eq!(*t.state(), StateName::Unavailable);
    }
    let mut t = reach(&[Begin, FailureLast]);
    assert!(t.consume(&ChooseIdentity).is_ok());
    assert_eq!(*t.state(), StateName::Starting);
}
