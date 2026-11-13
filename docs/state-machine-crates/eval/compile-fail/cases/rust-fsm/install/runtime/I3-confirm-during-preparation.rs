// I3 confirm-during-preparation. Driven through `consume`, every
// preparation stage's `Confirm` row (and `ConfirmWrong`'s) interrupts and
// fails: refused at run time.
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
        for i in [Confirm, ConfirmWrong] {
            let mut t = reach(&prep[..n]);
            assert_eq!(t.consume(&i).ok(), Some(Some(Output::InterruptFailed)));
            assert_eq!(*t.state(), State::Failed);
        }
    }
}
