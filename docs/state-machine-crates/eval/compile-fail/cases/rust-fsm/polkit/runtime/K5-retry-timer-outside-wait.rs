// K5 retry-timer-outside-wait. Driven through `consume`, only the rows into
// `RetryWait` arm the timer (`EndRetrying`, `Retrying`), and only
// `RetryWait` has a `RetryTimer` row: elsewhere `consume` returns `Err`.
// Refused at run time.
use impl_rust_fsm::polkit::fsm::{Input, Output, StateMachine};
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
    for path in [&[][..], &[Begin][..], &[Begin, Prompt][..], &[Begin, Prompt, Submit][..], &[Begin, FailureLast][..]] {
        let mut t = reach(path);
        assert_ne!(*t.state(), StateName::RetryWait);
        assert!(t.consume(&RetryTimer).is_err());
    }
    let mut t = reach(&[Begin]);
    assert_eq!(t.consume(&Failure).ok(), Some(Some(Output::EndRetrying)));
    assert_eq!(*t.state(), StateName::RetryWait);
}
