// L3 check-while-unlocked: submit the field for a PAM check while the lock
// is `Locking`. The `Check` graph is `Locked`'s data; `Locking` has only
// `confirm`. (`Checking` has no `submit` either: only `Idle` does.)
use impl_fsmentry::lock::{Lock, PhaseEntry, ScreenEntry};
use spec::lock::{Config, Event, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Lock, &mut out);
    assert_eq!(m.state().lock, LockPhase::Locking);
    if let ScreenEntry::Lockscreen(mut lockscreen) = m.screen_entry()
        && let PhaseEntry::Locking(locking) = lockscreen.as_mut().phase_entry()
    {
        locking.submit(1);
    }
}
