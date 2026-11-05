// L4 pending-sleep-after-resume: keep the pending sleep across `Resumed`, for a
// later confirmation to release. In the typed API
// `Inhibitor<SleepPending>::resumed` is the Resumed event, and it consumes the
// pending sleep (giving back the held delay); a new one needs
// `hold_for_sleep`, which is the next PrepareForSleep.
use impl_statum::lock::inhibitor::SomeState;
use impl_statum::lock::{Inhibitor, LockScreen, Locking, SleepPending};
use spec::lock::Effect;

fn resume_then_confirm(pending: Inhibitor<SleepPending>, locking: LockScreen<Locking>, out: &mut Vec<Effect>) {
    let _held = pending.resumed();
    let _ = locking.confirm(&mut SomeState::SleepPending(pending), out);
}

fn main() {}
