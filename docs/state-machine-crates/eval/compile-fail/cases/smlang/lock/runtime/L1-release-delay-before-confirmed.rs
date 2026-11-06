// L1 release-delay-before-confirmed. Only the lock region's `confirm` and
// leave actions send the delay region `Settled` (its `release` row); a
// second `PrepareForSleep` while `Locking` sends `Sleep(false)`, which
// `SleepPending` doesn't declare (refused at run time). The delay region's
// own events reach it only through `Machine::handle`.
use impl_smlang::lock::{DelayStates, Lock, LockEvents, LockStates};
use spec::lock::{Config, Effect, Event, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    let sm = &mut m;
    sm.process_event(&mut out, LockEvents::PrepareForSleep).unwrap();
    sm.process_event(&mut out, LockEvents::PrepareForSleep).unwrap();
    sm.process_event(&mut out, LockEvents::OutputAdded(1)).unwrap();
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert!(matches!(sm.state_enum(), LockStates::Locking));
    assert!(matches!(sm.context().delay_state(), DelayStates::SleepPending));
    sm.process_event(&mut out, LockEvents::LockConfirmed).unwrap();
    assert!(out.contains(&Effect::ReleaseDelay));
}
