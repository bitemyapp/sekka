// T3 lost-aside. Driven through `consume`, `Restoring` and `Disposing`
// have rows only for their own response (`Restored`/`RestoredCancel`,
// `Disposed`), whose actions restore or dispose of the aside; every other
// input is `Err`. Refused at run time. (The aside itself is data beside
// the table: the table's states can't hold it, and it stays readable,
// stale, after they're left.)
use impl_rust_fsm::transfer::fsm::{Input, StateMachine};
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
    let restoring = [Start, ProbedConflict, Replace, SetAside, PlacedSkip];
    let disposing = [Start, ProbedConflict, Replace, SetAside, PlacedOk];
    let others = [Start, ProbedFree, ProbedSame, Skip, Cancel, SetAsideFailed, PlacedOk, PlacedCancel, Listed, Exhausted];
    for path in [&restoring[..], &disposing[..]] {
        for i in others {
            let mut t = reach(path);
            let s = *t.state();
            assert!(matches!(s, StateName::Restoring | StateName::Disposing));
            assert!(t.consume(&i).is_err());
            assert_eq!(*t.state(), s);
        }
    }
    assert!(reach(&restoring).consume(&Disposed).is_err());
    assert!(reach(&disposing).consume(&Restored).is_err());
}
