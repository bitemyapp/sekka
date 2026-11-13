// L7 double-delay. Driven through `consume`, the `delay` table takes the
// delay only from `NotHeld`: `Take` and `Resumed` while `Held`, and `Take`
// while `SleepPending`, are `Err`. Refused at run time.
use impl_rust_fsm::lock::delay::{Input, StateMachine};
use spec::lock::DelayPhase;

fn main() {
    for i in [Input::Take, Input::Resumed] {
        let mut t = StateMachine::new();
        t.consume(&Input::Take).expect("a row");
        assert_eq!(*t.state(), DelayPhase::Held);
        assert!(t.consume(&i).is_err());
    }
    let mut t = StateMachine::new();
    t.consume(&Input::Take).expect("a row");
    t.consume(&Input::Sleep).expect("a row");
    assert!(t.consume(&Input::Take).is_err());
}
