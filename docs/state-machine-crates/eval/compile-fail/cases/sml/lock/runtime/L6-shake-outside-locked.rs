// L6 shake-outside-locked. The shake is `Locked`'s data, but a composite
// state has no payload: it lives in the private context. Only `refuse` (in
// `Checking`) starts it, only `state<Locked>`'s `ShakeTick` row (guarded by
// `shaking`) re-arms it, and `Locked`'s exit cancels it; elsewhere no row
// takes `ShakeTick`. Refused at run time.
use impl_sml::lock::{Lock, LockEvents as E, PamDone};
use spec::lock::{Config, Effect, Machine, PamOutcome};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    assert!(m.machine.process_event(&mut out, E::ShakeTick).is_err());
    for event in [E::Lock, E::LockConfirmed, E::Typed(false), E::Submit] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    let wrong = PamDone { check: 1, outcome: PamOutcome::WrongPassword };
    m.machine.process_event(&mut out, E::PamDone(wrong)).unwrap();
    m.machine.process_event(&mut out, E::LockEnded).unwrap();
    assert!(out.contains(&Effect::CancelShake));
    m.machine.process_event(&mut out, E::Lock).unwrap();
    out.clear();
    assert!(m.machine.process_event(&mut out, E::ShakeTick).is_err());
    m.machine.process_event(&mut out, E::LockConfirmed).unwrap();
    assert!(m.machine.process_event(&mut out, E::ShakeTick).is_err());
    assert!(!out.iter().any(|e| matches!(e, Effect::ArmShake | Effect::ShakeStep(_))));
}
