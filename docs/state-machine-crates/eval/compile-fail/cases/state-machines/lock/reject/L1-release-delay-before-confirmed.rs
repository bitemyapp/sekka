// L1 release-delay-before-confirmed: the one typed release of a pending
// sleep's delay is `Delay<SleepPending>::lock_settled`, which stands for
// "lock confirmed (or lock screen left)"; the other release, a sleep while
// locked, is `Delay<Held>::sleep`, and a pending sleep's delay isn't a
// `Delay<Held>`. No type ties `lock_settled` to the `Locker`: only
// `Lock::handle`'s relay sends it, after the lock region left `Locking`.
use impl_state_machines::lock::{Delay, DelayCtx, DelayHeldSleepOutcome, Held};
use spec::lock::{Config, LockPhase};

fn main() {
    let held = Delay::new(DelayCtx::new(Config::default())).acquire().unwrap();
    let DelayHeldSleepOutcome::SleepPending(pending) = held.sleep(LockPhase::Locking).unwrap() else { return };
    let _ = Delay::<Held>::sleep(pending, LockPhase::Locked);
}
