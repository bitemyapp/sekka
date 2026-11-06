// L3 check-while-unlocked. `Submit` matches only `Phase::Locked { has_text:
// true, .. }`: while unlocked, locking or checking it is dropped (refused at
// run time).
use impl_enum::lock::Lock;
use spec::lock::{Config, Effect, Event, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Typed { empty: false }, &mut out);
    m.handle(Event::Submit, &mut out);
    m.handle(Event::Lock, &mut out);
    m.handle(Event::Typed { empty: false }, &mut out);
    m.handle(Event::Submit, &mut out);
    assert_eq!(m.state().lock, LockPhase::Locking);
    assert!(!out.iter().any(|e| matches!(e, Effect::StartCheck { .. })));
    for event in [Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit, Event::Typed { empty: false }, Event::Submit] {
        m.handle(event, &mut out);
    }
    assert_eq!(m.state().lock, LockPhase::Checking);
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::StartCheck { .. })).count(), 1);
}
