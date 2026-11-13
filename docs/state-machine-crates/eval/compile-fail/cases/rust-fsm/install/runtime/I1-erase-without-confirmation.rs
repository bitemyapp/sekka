// I1 erase-without-confirmation. Driven through `consume`, the only rows
// into `Erase` are `Prepared`'s and `PreparedDeferred`'s `Confirm`; a
// preparation stage fails on `Confirm`, and `Prepared` has no other row
// onwards: refused at run time.
use impl_rust_fsm::install::fsm::{Input, Output, State, StateMachine};

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    use Input::*;
    let prep = [Begin, Checked, Configured, EvaluatedFits, Built];
    for n in 1..=prep.len() {
        let mut t = reach(&prep[..n]);
        assert_eq!(t.consume(&Confirm).ok(), Some(Some(Output::InterruptFailed)));
        assert_eq!(*t.state(), State::Failed);
    }
    let prepared = [Begin, Checked, Configured, EvaluatedFits, Built, Cached];
    let deferred = [Begin, Checked, Configured, EvaluatedDeferred];
    for path in [&prepared[..], &deferred[..]] {
        for i in [StepDone, Cached, Begin] {
            let mut t = reach(path);
            assert!(matches!(t.state(), State::Prepared | State::PreparedDeferred));
            assert!(t.consume(&i).is_err());
        }
        let mut t = reach(path);
        assert_eq!(t.consume(&ConfirmWrong).ok(), Some(Some(Output::Failed)));
    }
}
