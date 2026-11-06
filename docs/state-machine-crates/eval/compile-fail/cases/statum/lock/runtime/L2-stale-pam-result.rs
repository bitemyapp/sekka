// L2 stale-pam-result. `LockScreen<Checking>::accepted` takes no check id: the
// runtime wrapper compares the result's with the payload's `check()` and
// drops an earlier lock's.
use impl_statum::lock::Lock;
use impl_statum::lock::lock_screen::SomeState;
use spec::lock::{Config, Effect, Event, LockPhase, Machine, PamOutcome};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    for event in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit, Event::UnlockRequested] {
        m.handle(event, &mut out);
    }
    for event in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit] {
        m.handle(event, &mut out);
    }
    assert!(out.contains(&Effect::StartCheck { check: 2 }));
    out.clear();
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::Accepted }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state().lock, LockPhase::Checking);
    let (SomeState::Checking(c), _) = m.typed() else { panic!("not checking") };
    assert_eq!(c.state_data.check(), 2);
}
