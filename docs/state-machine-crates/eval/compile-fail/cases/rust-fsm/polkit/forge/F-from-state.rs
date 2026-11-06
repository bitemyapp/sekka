// Forge: `StateMachine::from_state`. The crate generates it public on the
// same type that carries the transition API (`consume`), with no seal. A
// fresh table forged into `Prompted` (no prompt was ever shown) writes an
// answer on `Submit`, committing K1 (answer-without-prompt) at the table
// level. It can't reach `Polkit`'s own table, which is private. Other
// means: a bare table's `consume` takes any declared edge without
// `input()`'s conversation and cookie guards (K2 has no conversation to
// check there).
use impl_rust_fsm::polkit::fsm::{Input, Output, StateMachine};
use spec::polkit::StateName;

fn main() {
    let mut t = StateMachine::from_state(StateName::Prompted);
    assert_eq!(t.consume(&Input::Submit).ok(), Some(Some(Output::WriteAnswer)));
    assert_eq!(*t.state(), StateName::Checking);
}
