// T4 place-without-answer. Driven through `consume`, a conflict leads the
// table to `Asking` (output `Ask`), and `Asking` has no `Placed…` row: only
// an answer leaves it. Refused at run time.
use impl_rust_fsm::transfer::fsm::{Input, Output, StateMachine};
use spec::transfer::StateName;

fn main() {
    let mut t = StateMachine::new();
    t.consume(&Input::Start).expect("a row");
    assert_eq!(t.consume(&Input::ProbedConflict).ok(), Some(Some(Output::Ask)));
    assert_eq!(*t.state(), StateName::Asking);
    for i in [Input::PlacedOk, Input::PlacedSkip, Input::SetAside, Input::Disposed] {
        assert!(t.consume(&i).is_err());
        assert_eq!(*t.state(), StateName::Asking);
    }
}
