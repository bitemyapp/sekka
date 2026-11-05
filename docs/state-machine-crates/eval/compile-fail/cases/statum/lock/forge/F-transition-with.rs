// Forge, L3 check-while-unlocked. `confirm` declares the edge
// `Locking → Locked`, so statum implements the public
// `CanTransitionWith<LockedData>` for `LockScreen<Locking>`, and taking it
// skips `confirm`'s body (no `LockedHint(true)`, no settling of a pending
// sleep). Outside code can't make a `LockedData` (private fields), but
// `state_data` is always a public field: it moves one out of another lock
// screen that was legitimately confirmed and typed into. With the
// compositor's lock never confirmed, a PAM check starts. (The other public
// means: `transition_map` and `CanTransitionMap`. L6 the same way, with a
// shaking `LockedData`; L1 with a `HeldTok` taken into an
// `Inhibitor<SleepPending>`, whose `Held` then releases.)
use impl_statum::lock::inhibitor::SomeState;
use impl_statum::lock::{Inhibitor, LockScreen, Locked, Unlocked};
use spec::lock::{Config, Effect};
use statum::CanTransitionWith;

fn main() {
    let mut donor_out: Vec<Effect> = Vec::new();
    let mut delay = SomeState::NotHeld(Inhibitor::new());
    let donor = LockScreen::<Unlocked>::new(Config::default()).lock(&mut donor_out);
    let mut donor = donor.confirm(&mut delay, &mut donor_out);
    donor.typed(false);
    let mut out: Vec<Effect> = Vec::new();
    let locking = LockScreen::<Unlocked>::new(Config::default()).lock(&mut out);
    let locked: LockScreen<Locked> = locking.transition_with_data(donor.state_data);
    assert!(locked.submit(&mut out).is_ok());
    assert_eq!(out, [Effect::RequestLock, Effect::OpenSurface(0), Effect::StartCheck { check: 1 }]);
}
