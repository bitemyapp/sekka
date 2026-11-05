// K3 restart-without-backoff. Driven through `consume`, an unanswered
// failure (`Failure` in `Starting`/`Prompted`, `Unreachable` in
// `Starting`) leads only to `RetryWait` or `Unavailable`, never straight
// to `Starting`: refused at run time.
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
    for (path, i) in [
        (&[Begin][..], Failure),
        (&[Begin, Prompt][..], Failure),
        (&[Begin][..], Unreachable),
        (&[Begin][..], FailureLast),
        (&[Begin, Prompt][..], FailureLast),
        (&[Begin][..], UnreachableLast),
    ] {
        let mut t = reach(path);
        assert!(t.consume(&i).is_ok());
        assert!(matches!(t.state(), StateName::RetryWait | StateName::Unavailable));
    }
}
