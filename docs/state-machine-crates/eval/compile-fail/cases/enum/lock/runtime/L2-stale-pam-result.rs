// L2 stale-pam-result. `handle` acts on `PamDone` only when its check equals
// `Phase::Checking`'s: the first lock's result is dropped during the second
// lock's check (refused at run time).
use impl_enum::lock::{Lock, Phase};
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
    assert!(matches!(m.typed().0, Phase::Checking { check: 2, .. }));
}
