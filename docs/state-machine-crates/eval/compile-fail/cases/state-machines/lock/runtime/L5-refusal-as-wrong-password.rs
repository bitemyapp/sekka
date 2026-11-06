// L5 refusal-as-wrong-password. The typed `pam_done` with a refusal from
// `Checking` runs `show_refusal`, which reports PAM's reason (SHB-12;
// refused at run time: the callback maps the outcome).
use impl_state_machines::lock::{Locker, LockerCheckingPamDoneOutcome, LockerCtx};
use spec::lock::{Config, Effect, LockNote, PamOutcome, Refusal};

fn main() {
    let idle = Locker::new(LockerCtx::new(Config::default())).lock().unwrap().lock_confirmed().unwrap();
    let mut checking = idle.typed(false).unwrap().submit().unwrap();
    checking.take_effects();
    let refused = PamOutcome::Refused(Refusal::AccountLocked);
    let LockerCheckingPamDoneOutcome::Idle(mut idle) = checking.pam_done((1, refused)).unwrap() else { panic!() };
    let effects = idle.take_effects();
    assert_eq!(effects[0], Effect::ShowNote(LockNote::Refused(Refusal::AccountLocked)));
    assert!(!effects.contains(&Effect::ShowNote(LockNote::WrongPassword)));
}
