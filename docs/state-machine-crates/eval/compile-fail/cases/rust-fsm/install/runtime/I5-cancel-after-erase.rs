// I5 cancel-after-erase. Driven through `consume`, from `Erase` onwards
// the table has no `Cancel` row (`Err`), and `Cancelling` is reached only
// from preparation stages: refused at run time.
use impl_rust_fsm::install::fsm::{Input, StateMachine};

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    use Input::*;
    let plain = [Begin, Checked, Configured, EvaluatedFits, Built, Cached, Confirm, StepDone, StepDone, StepDone, StepDone, StepDone, StepDone];
    let deferred = [Begin, Checked, Configured, EvaluatedDeferred, Confirm, StepDone, StepDone, StepDone];
    // Erase, Format, Files, Copy, Bootloader, Record, Flush; then the deferred
    // route's Erase, Format, Files, BuildOnTarget.
    for path in (7..=plain.len()).map(|n| &plain[..n]).chain((5..=deferred.len()).map(|n| &deferred[..n])) {
        let mut t = reach(path);
        let s = *t.state();
        assert!(t.consume(&Cancel).is_err());
        assert_eq!(*t.state(), s);
    }
}
