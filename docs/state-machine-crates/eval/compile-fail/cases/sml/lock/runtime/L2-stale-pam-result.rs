// L2 stale-pam-result. `Checking`'s payload is its check, and the PAM rows'
// guards compare it: the first lock's result fails them during the second
// lock's check (`Err`). Refused at run time.
use impl_sml::lock::{Lock, LockEvents as E, LockLockedStates, PamDone};
use spec::lock::{Config, Effect, LockPhase, Machine, PamOutcome};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    for event in [E::Lock, E::LockConfirmed, E::Typed(false), E::Submit, E::UnlockRequested] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    for event in [E::Lock, E::LockConfirmed, E::Typed(false), E::Submit] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    assert!(out.contains(&Effect::StartCheck { check: 2 }));
    out.clear();
    let stale = PamDone { check: 1, outcome: PamOutcome::Accepted };
    assert!(m.machine.process_event(&mut out, E::PamDone(stale)).is_err());
    assert!(out.is_empty());
    assert!(m.machine.is(&LockLockedStates::Checking(0)));
    assert_eq!(m.state().lock, LockPhase::Checking);
}
