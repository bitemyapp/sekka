// T1 dispose-before-placed. Driven through `consume`, only
// `PlacingReplacement`'s `PlacedOk` row leads to `Dispose`; the states
// before it have no such row: refused at run time.
use impl_rust_fsm::transfer::fsm::{Input, Output, StateMachine};
use spec::transfer::StateName;

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    use Input::*;
    for path in [&[Start, ProbedConflict][..], &[Start, ProbedConflict, Replace][..]] {
        for i in [PlacedOk, Disposed, SetAside] {
            let mut t = reach(path);
            assert!(matches!(t.state(), StateName::Asking | StateName::SettingAside));
            assert_ne!(t.consume(&i).ok(), Some(Some(Output::Dispose)));
            assert_ne!(*t.state(), StateName::Disposing);
        }
    }
    let mut t = reach(&[Start, ProbedConflict, Replace, SetAside]);
    assert_eq!(*t.state(), StateName::PlacingReplacement);
    assert!(t.consume(&Disposed).is_err());
}
