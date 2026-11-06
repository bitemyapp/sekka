// L5 refusal-as-wrong-password. The table can't carry the reason: its
// `WrongPassword` and `Refused` rows both name the `Refused` action, which
// reads the reason from the event: refused at run time (by the action).
use impl_rust_fsm::lock::Lock;
use spec::lock::{Config, Effect, Event, LockNote, Machine, PamOutcome, Refusal};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Lock, &mut out);
    m.handle(Event::LockConfirmed, &mut out);
    m.handle(Event::Typed { empty: false }, &mut out);
    m.handle(Event::Submit, &mut out);
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::Refused(Refusal::AccountLocked) }, &mut out);
    assert!(out.contains(&Effect::ShowNote(LockNote::Refused(Refusal::AccountLocked))));
    assert!(!out.contains(&Effect::ShowNote(LockNote::WrongPassword)));
}
