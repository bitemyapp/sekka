// L2 stale-pam-result. The table has no check ids (`Accepted` is any
// acceptance); `handle`'s input function drops a result whose check isn't
// the newest: refused at run time.
use impl_rust_fsm::lock::Lock;
use spec::lock::{Config, Event, LockPhase, Machine, PamOutcome};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Lock, &mut out);
    m.handle(Event::LockConfirmed, &mut out);
    m.handle(Event::Typed { empty: false }, &mut out);
    m.handle(Event::Submit, &mut out);
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::WrongPassword }, &mut out);
    m.handle(Event::Typed { empty: false }, &mut out);
    m.handle(Event::Submit, &mut out);
    out.clear();
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::Accepted }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state().lock, LockPhase::Checking);
}
