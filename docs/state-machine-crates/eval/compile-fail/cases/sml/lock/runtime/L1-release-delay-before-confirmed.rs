// L1 release-delay-before-confirmed. While `Locking`, `SleepPending` releases
// only on `LockConfirmed` (which confirms) or on leaving the lock screen: a
// second sleep, a new output or typing release nothing. The only other
// release, `Held + event<SleepWhileLocked>`, is processed by `Locked` alone,
// and outside code can't build that event (private field). Refused at run
// time.
use impl_sml::lock::{Lock, LockEvents as E};
use spec::lock::{Config, DelayPhase, Effect, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    for event in [E::SessionStarted, E::PrepareForSleep, E::PrepareForSleep, E::OutputAdded(1)] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    assert!(m.machine.process_event(&mut out, E::Typed(false)).is_err());
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert_eq!(m.state().lock, LockPhase::Locking);
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    m.machine.process_event(&mut out, E::LockConfirmed).unwrap();
    assert_eq!(out[out.len() - 2..], [Effect::LockedHint(true), Effect::ReleaseDelay]);
}
