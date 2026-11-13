// I2 bootloader-before-copy. Driven through `consume`, only `Copy` and
// `BuildOnTarget` lead to `Bootloader`; `Files` (either route) goes to
// step 4: refused at run time.
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
    let plain = [Begin, Checked, Configured, EvaluatedFits, Built, Cached, Confirm, StepDone, StepDone];
    let deferred = [Begin, Checked, Configured, EvaluatedDeferred, Confirm, StepDone, StepDone];
    let mut t = reach(&plain);
    assert_eq!(*t.state(), State::Files);
    assert_eq!(t.consume(&StepDone).ok(), Some(Some(Output::Copy)));
    let mut t = reach(&deferred);
    assert_eq!(*t.state(), State::FilesDeferred);
    assert_eq!(t.consume(&StepDone).ok(), Some(Some(Output::BuildOnTarget)));
    for path in [&plain[..7], &plain[..8], &deferred[..5], &deferred[..6]] {
        let mut t = reach(path);
        assert_ne!(t.consume(&StepDone).ok(), Some(Some(Output::Bootloader)));
    }
}
