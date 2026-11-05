// Forge: `StateMachine::from_state`. The crate generates it public on the
// same type that carries the transition API (`consume`), with no seal. A
// table driven into `Erase`, then replaced by one forged into
// `Cancelling`, ends the session `Cancelled` on the step's result after
// the disk was written: I5 (cancel-after-erase) at the table level. It
// can't reach `Install`'s own table, which is private. Other means: a bare
// table's `consume(&Input::EvaluatedDeferred)` takes the deferred route
// without `input()`'s room check (I4).
use impl_rust_fsm::install::fsm::{Input, Output, State, StateMachine};

fn main() {
    let mut t = StateMachine::new();
    for i in [Input::Begin, Input::Checked, Input::Configured, Input::EvaluatedDeferred, Input::Confirm] {
        t.consume(&i).expect("a row");
    }
    assert_eq!(*t.state(), State::EraseDeferred);
    t = StateMachine::from_state(State::Cancelling);
    assert_eq!(t.consume(&Input::StepDone).ok(), Some(Some(Output::Cancelled)));
    assert_eq!(*t.state(), State::Cancelled);
}
