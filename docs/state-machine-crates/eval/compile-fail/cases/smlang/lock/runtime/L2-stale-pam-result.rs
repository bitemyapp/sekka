// L2 stale-pam-result. `Checking`'s `PamDone` rows are guarded by `checked`
// (the result's check is the one `Checking` carries); an earlier lock's
// result passes no guard: `Err(TransitionsFailed)` (refused at run time).
use impl_smlang::lock::{Lock, LockError, LockEvents, LockStates};
use spec::lock::{Config, Effect, Machine, PamOutcome};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    let check = [LockEvents::Lock, LockEvents::LockConfirmed, LockEvents::Typed(false), LockEvents::Submit];
    for e in check {
        sm.process_event(&mut out, e).unwrap();
    }
    sm.process_event(&mut out, LockEvents::UnlockRequested).unwrap();
    for e in check {
        sm.process_event(&mut out, e).unwrap();
    }
    assert!(matches!(sm.state_enum(), LockStates::Checking(c) if c.check == 2));
    out.clear();
    let stale = sm.process_event(&mut out, LockEvents::PamDone((1, PamOutcome::Accepted)));
    assert_eq!(stale, Err(LockError::TransitionsFailed));
    assert!(!out.contains(&Effect::ReleaseLock));
    assert!(matches!(sm.state_enum(), LockStates::Checking(_)));
}
