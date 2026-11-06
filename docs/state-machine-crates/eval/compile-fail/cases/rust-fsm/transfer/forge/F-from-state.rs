// Forge: `StateMachine::from_state`. The crate generates it public on the
// same type that carries the transition API (`consume`), with no seal. A
// table driven into `Restoring` (an aside is out) and then replaced by one
// forged into `Probing` leaves `Restoring` without restoring the aside,
// and the job goes on: T3 (lost-aside) at the table level. It can't reach
// `Transfer`'s own table, which is private. Other means: a bare table's
// `consume` takes any declared edge without `input()`'s item and
// `can_merge` guards.
use impl_rust_fsm::transfer::fsm::{Input, Output, StateMachine};
use spec::transfer::StateName;

fn main() {
    let mut t = StateMachine::new();
    for i in [Input::Start, Input::ProbedConflict, Input::Replace, Input::SetAside, Input::PlacedSkip] {
        t.consume(&i).expect("a row");
    }
    assert_eq!(*t.state(), StateName::Restoring);
    t = StateMachine::from_state(StateName::Probing);
    assert_eq!(t.consume(&Input::ProbedSame).ok(), Some(Some(Output::Done)));
    assert_eq!(*t.state(), StateName::Probing);
}
