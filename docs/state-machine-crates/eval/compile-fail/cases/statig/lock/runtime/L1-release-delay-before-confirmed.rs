// L1 release-delay-before-confirmed. The delay region's `sleep_pending`
// releases only once the lock region (its context's `lock`) is no longer
// `Locking`; a second PrepareForSleep or a new output changes nothing
// (refused at run time).
use impl_statig::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    m.handle(Event::OutputAdded(1), &mut out);
    m.handle(Event::Typed { empty: false }, &mut out);
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert_eq!(m.state().lock, LockPhase::Locking);
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    m.handle(Event::LockConfirmed, &mut out);
    assert_eq!(out[out.len() - 2..], [Effect::LockedHint(true), Effect::ReleaseDelay]);
}
