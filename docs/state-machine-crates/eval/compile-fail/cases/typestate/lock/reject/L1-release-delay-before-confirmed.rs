// L1 release-delay-before-confirmed: let logind sleep while the lock is
// still `Locking`. Once held for a sleep, the delay is a `PendingSleep`,
// not a `DelayFd` with its public `release`: only `Locking::confirm` and
// the ways out of the lock screen release it.
use impl_typestate::lock::{DelayFd, Locking, NoDelay, PendingSleep};
use spec::lock::Effect;

fn sleep_now(locking: Locking, pending: PendingSleep, out: &mut Vec<Effect>) -> (Locking, NoDelay) {
    (locking, DelayFd::release(pending, out))
}

fn main() {}
