// L1 release-delay-before-confirmed. SPEC-FIX: `PrepareForSleep` while
// locking sets `sleep_pending` instead of letting logind sleep at once; only
// the confirmation (or leaving) releases it (refused at run time).
use impl_implicit::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    m.handle(Event::OutputAdded(1), &mut out);
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert_eq!(m.state().lock, LockPhase::Locking);
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    m.handle(Event::LockConfirmed, &mut out);
    assert!(out.contains(&Effect::ReleaseDelay));
}
