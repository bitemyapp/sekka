// L1 release-delay-before-confirmed. The two regions are two tables; the
// `delay` table releases a pending sleep on `Settled`, which only the
// `screen` table's confirm and leave actions feed it (a bare `delay`
// table would take `Settled` whatever the lock screen does). While
// `Locking`, nothing releases it: refused at run time.
use impl_rust_fsm::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    m.handle(Event::OutputAdded(1), &mut out);
    m.handle(Event::Submit, &mut out);
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert_eq!(m.state().lock, LockPhase::Locking);
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    m.handle(Event::LockConfirmed, &mut out);
    assert!(out.contains(&Effect::ReleaseDelay));
}
