// L3 check-while-unlocked. Only `Locked`'s `Idle` has a `Submit` row (guarded
// by the text); in `Unlocked`, `Locking` and `Checking` no row takes it.
// Refused at run time.
use impl_sml::lock::{Lock, LockEvents as E};
use spec::lock::{Config, Effect, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    assert!(m.machine.process_event(&mut out, E::Typed(false)).is_err());
    assert!(m.machine.process_event(&mut out, E::Submit).is_err());
    m.machine.process_event(&mut out, E::Lock).unwrap();
    assert!(m.machine.process_event(&mut out, E::Submit).is_err());
    assert_eq!(m.state().lock, LockPhase::Locking);
    for event in [E::LockConfirmed, E::Typed(false), E::Submit] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    assert!(m.machine.process_event(&mut out, E::Typed(false)).is_err());
    assert!(m.machine.process_event(&mut out, E::Submit).is_err());
    assert_eq!(m.state().lock, LockPhase::Checking);
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::StartCheck { .. })).count(), 1);
}
