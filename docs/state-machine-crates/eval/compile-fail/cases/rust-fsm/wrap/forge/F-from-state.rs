// Forge: `StateMachine::from_state`. The crate generates it public on the
// same type that carries the transition API (`consume`), with no seal. A
// fresh table (the cursor would be at column 0) forged into `PendingWrap`
// soft-wraps on the first print, committing W1 (pending-off-the-edge) at
// the table level. It can't reach `Wrap`'s own table, which is private.
// Other means: a bare table's `consume(&Input::Reached)` sets the pending
// flag in any column (no cursor exists there).
use impl_rust_fsm::wrap::fsm::{Input, Output, StateMachine};
use spec::wrap::StateName;

fn main() {
    let mut t = StateMachine::from_state(StateName::PendingWrap);
    assert_eq!(t.consume(&Input::Print).ok(), Some(Some(Output::WrapPrint)));
}
