// L4 pending-sleep-after-resume. Driven through `consume`, the `delay`
// table's `Resumed` row takes `SleepPending` back to `Held`: refused at run
// time.
use impl_rust_fsm::lock::delay::{Input, StateMachine};
use spec::lock::DelayPhase;

fn main() {
    let mut t = StateMachine::new();
    t.consume(&Input::Take).expect("a row");
    t.consume(&Input::Sleep).expect("a row");
    assert_eq!(*t.state(), DelayPhase::SleepPending);
    assert_eq!(t.consume(&Input::Resumed).ok(), Some(None));
    assert_eq!(*t.state(), DelayPhase::Held);
}
