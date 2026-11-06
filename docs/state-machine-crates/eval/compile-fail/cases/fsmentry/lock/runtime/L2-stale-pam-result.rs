// L2 stale-pam-result. The check id is `Checking`'s data and compared in
// `handle`; an earlier lock's check 1 does nothing to check 2 (refused at
// run time).
use impl_fsmentry::lock::Lock;
use spec::lock::{Config, Event, LockPhase, Machine, PamOutcome};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    for e in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit, Event::UnlockRequested] {
        m.handle(e, &mut out);
    }
    for e in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit] {
        m.handle(e, &mut out);
    }
    assert_eq!(m.state().lock, LockPhase::Checking);
    out.clear();
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::Accepted }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state().lock, LockPhase::Checking);
}
