// W3 clear-on-tab. Driven through `consume`, the table's `Tab` rows keep
// both pending states: refused at run time.
use impl_rust_fsm::wrap::fsm::{Input, Output, StateMachine};
use spec::wrap::StateName;

fn main() {
    for path in [&[Input::Reached][..], &[Input::Reached, Input::AutowrapOff][..]] {
        let mut t = StateMachine::new();
        for i in path {
            t.consume(i).expect("a row of the table");
        }
        let s = *t.state();
        assert!(matches!(s, StateName::PendingWrap | StateName::PendingNoAutowrap));
        assert_eq!(t.consume(&Input::Tab).ok(), Some(Some(Output::Tab)));
        assert_eq!(*t.state(), s);
    }
}
