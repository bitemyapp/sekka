// L5 refusal-as-wrong-password. `checking` maps `Refused(r)` to
// `LockNote::Refused(r)`; nothing in the types stops a `WrongPassword`
// there, but it doesn't happen (refused at run time).
use impl_statig::lock::Lock;
use spec::lock::{Config, Effect, Event, LockNote, Machine, PamOutcome, Refusal};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    for event in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit] {
        m.handle(event, &mut out);
    }
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::Refused(Refusal::AccountLocked) }, &mut out);
    assert!(out.contains(&Effect::ShowNote(LockNote::Refused(Refusal::AccountLocked))));
    assert!(!out.contains(&Effect::ShowNote(LockNote::WrongPassword)));
}
