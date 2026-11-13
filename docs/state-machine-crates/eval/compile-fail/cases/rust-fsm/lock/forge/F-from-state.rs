// Forge: `StateMachine::from_state`. The crate generates it public on the
// same type that carries the transition API (`consume`), with no seal. A
// `delay` table resumed to `Held`, then replaced by one forged back into
// `SleepPending`, releases the fresh delay on the next `Settled` (a late
// lock confirmation): L4 (pending-sleep-after-resume) at the table level.
// It can't reach `Lock`'s own tables, which are private. Other means: a
// bare `delay` table takes `Settled` whatever the screen region does (L1),
// and either table's `consume` skips `input()`'s guards.
use impl_rust_fsm::lock::delay::{Input, Output, StateMachine};
use spec::lock::DelayPhase;

fn main() {
    let mut t = StateMachine::new();
    for i in [Input::Take, Input::Sleep, Input::Resumed] {
        t.consume(&i).expect("a row");
    }
    assert_eq!(*t.state(), DelayPhase::Held);
    t = StateMachine::from_state(DelayPhase::SleepPending);
    assert_eq!(t.consume(&Input::Settled).ok(), Some(Some(Output::ReleaseDelay)));
}
