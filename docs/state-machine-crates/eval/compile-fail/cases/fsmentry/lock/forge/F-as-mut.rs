// F-as-mut (lock): commits L2 stale-pam-result. fsmentry generates
// `AsMut<LockscreenData>` on the `Lockscreen` handle, public with the
// handles: swapping two live machines' lock screens puts check 1 in `b`
// while its own check 2 runs, and the stale `PamDone { check: 1 }` unlocks
// it. (`LockscreenData` has private fields and isn't `Copy`, so a swap is
// the only write. Other crate-generated means: `AsMut<u32>` on `Checking`,
// the `&mut LockedData` that the sink state `Locked` hands out, and every
// exit handing back its data for another machine's edge to take.)
use impl_fsmentry::lock::{Lock, ScreenEntry};
use spec::lock::{Config, Event, LockPhase, Machine, PamOutcome};

fn main() {
    let (mut a, mut b) = (Lock::new(Config::default()), Lock::new(Config::default()));
    let mut out = Vec::new();
    for e in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit] {
        a.handle(e, &mut out);
    }
    let wrong = Event::PamDone { check: 1, outcome: PamOutcome::WrongPassword };
    for e in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit, wrong] {
        b.handle(e, &mut out);
    }
    for e in [Event::Typed { empty: false }, Event::Submit] {
        b.handle(e, &mut out);
    }
    assert_eq!(b.state().lock, LockPhase::Checking);
    if let (ScreenEntry::Lockscreen(mut x), ScreenEntry::Lockscreen(mut y)) = (a.screen_entry(), b.screen_entry()) {
        std::mem::swap(x.as_mut(), y.as_mut());
    }
    b.handle(Event::PamDone { check: 1, outcome: PamOutcome::Accepted }, &mut out);
    assert_eq!(b.state().lock, LockPhase::Unlocked);
}
