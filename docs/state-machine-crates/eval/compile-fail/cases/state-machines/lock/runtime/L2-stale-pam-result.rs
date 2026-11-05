// L2 stale-pam-result. The typed `pam_done` from `Checking` has the
// event-level `current` guard: check 1's result while check 2 runs is
// refused and the machine handed back (refused at run time;
// `forge/F-checking-data-mut.rs` defeats it by rewriting the check id).
use impl_state_machines::lock::{Locker, LockerCheckingPamDoneOutcome, LockerCtx};
use spec::lock::{Config, PamOutcome};

fn main() {
    let idle = Locker::new(LockerCtx::new(Config::default())).lock().unwrap().lock_confirmed().unwrap();
    let checking = idle.typed(false).unwrap().submit().unwrap();
    let LockerCheckingPamDoneOutcome::Idle(idle) = checking.pam_done((1, PamOutcome::WrongPassword)).unwrap() else {
        panic!()
    };
    let mut checking = idle.typed(false).unwrap().submit().unwrap();
    assert_eq!(checking.checking_data(), Some(&2));
    checking.take_effects();
    let Err((mut checking, _)) = checking.pam_done((1, PamOutcome::Accepted)) else { panic!("a stale result was taken") };
    assert!(checking.take_effects().is_empty());
}
