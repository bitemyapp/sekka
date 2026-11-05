// Forge (L2 stale-pam-result). The generated, public `checking_data_mut`
// rewrites the running check's id: check 1's Accepted then unlocks while
// check 2 runs. Other means that stay public: `with_<state>_data`,
// `<state>_data_mut` and the generic `state_data_<state>_mut` on every typed
// machine (the field, the shake); `set_<state>_data` and `<state>_data_mut`
// on the wrapper `into_dynamic()` returns; and `initialize()` on every state
// type, which re-runs the active entry hooks: on a `Locker<Idle>` it emits
// RequestLock, OpenSurface(o) and LockedHint(true) again.
use impl_state_machines::lock::{Locker, LockerCheckingPamDoneOutcome, LockerCtx};
use spec::lock::{Config, Effect, PamOutcome};

fn main() {
    let idle = Locker::new(LockerCtx::new(Config::default())).lock().unwrap().lock_confirmed().unwrap();
    let checking = idle.typed(false).unwrap().submit().unwrap();
    let LockerCheckingPamDoneOutcome::Idle(idle) = checking.pam_done((1, PamOutcome::WrongPassword)).unwrap() else {
        panic!()
    };
    let mut checking = idle.typed(false).unwrap().submit().unwrap();
    assert_eq!(checking.checking_data(), Some(&2));
    *checking.checking_data_mut().unwrap() = 1;
    checking.take_effects();
    let LockerCheckingPamDoneOutcome::Unlocked(mut unlocked) = checking.pam_done((1, PamOutcome::Accepted)).unwrap() else {
        panic!()
    };
    assert!(unlocked.take_effects().contains(&Effect::ReleaseLock));
}
