// L4 pending-sleep-after-resume: keep the pending sleep across `Resumed`,
// for a later confirmation to release. In the typed API
// `PendingSleep::resumed` is the Resumed event, and it consumes the pending
// sleep (giving back the held `DelayFd`); a new one needs `hold_for_sleep`,
// which is the next PrepareForSleep.
use impl_typestate::lock::{Delay, Locking, PendingSleep};
use spec::lock::Effect;

fn resume_then_confirm(pending: PendingSleep, locking: Locking, out: &mut Vec<Effect>) {
    let _held = pending.resumed();
    let _ = locking.confirm(Delay::SleepPending(pending), out);
}

fn main() {}
