// L1 release-delay-before-confirmed. The two regions are separate graphs,
// and `Delay`'s `SleepPending -release-> NotHeld` is also LogindVanished's
// edge (one edge per pair of states), so it can't stand for the lock's
// confirmation. Through the live machine's handles the pending sleep's
// delay is dropped while still `Locking`.
use impl_fsmentry::lock::{DelayEntry, Lock};
use spec::lock::{Config, DelayPhase, Event, LockPhase, Machine, StateName};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    assert_eq!(m.state(), StateName { lock: LockPhase::Locking, delay: DelayPhase::SleepPending });
    if let DelayEntry::SleepPending(pending) = m.delay_entry() {
        pending.release();
    }
    assert_eq!(m.state(), StateName { lock: LockPhase::Locking, delay: DelayPhase::NotHeld });
}
