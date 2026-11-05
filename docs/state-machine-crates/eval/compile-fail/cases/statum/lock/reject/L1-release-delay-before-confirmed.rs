// L1 release-delay-before-confirmed: let logind sleep while the lock is still
// `Locking`. Once held for a sleep the delay is an `Inhibitor<SleepPending>`,
// a state of its own whose `release` is private: only
// `LockScreen<Locking>::confirm` and the ways out of the lock screen release
// it (through `settle`). It is not an `Inhibitor<Held>` with that state's
// public `release`.
use impl_statum::lock::{Held, Inhibitor, LockScreen, Locking, NotHeld, SleepPending};
use spec::lock::Effect;

fn sleep_now(
    locking: LockScreen<Locking>,
    pending: Inhibitor<SleepPending>,
    out: &mut Vec<Effect>,
) -> (LockScreen<Locking>, Inhibitor<NotHeld>) {
    (locking, Inhibitor::<Held>::release(pending, out))
}

fn main() {}
