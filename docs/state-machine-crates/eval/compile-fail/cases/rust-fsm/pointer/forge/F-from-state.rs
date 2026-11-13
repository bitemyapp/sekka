// Forge: `StateMachine::from_state`. The crate generates it public on the
// same type that carries the transition API (`consume`), with no seal, so
// it can't be hidden without hiding the table. It builds the generated
// machine in any state: a fresh table forged into `Pressed` (no press ever
// happened) maps the next release to `Up`, committing P1
// (release-without-owner) at the table level. It can't reach `Pointer`'s
// own table, which is private. Other means: a bare table's `consume` takes
// any declared edge without `input()`'s guards or `act()`'s actions.
use impl_rust_fsm::pointer::fsm::{Input, Output, StateMachine};
use spec::pointer::StateName;

fn main() {
    let mut t = StateMachine::from_state(StateName::Pressed);
    assert_eq!(t.consume(&Input::Release).ok(), Some(Some(Output::Up)));
    assert_eq!(*t.state(), StateName::Hovering);
}
