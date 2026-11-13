// L5 refusal-as-wrong-password. BUG(SHB-12): whatever PAM refused for, the
// note is "That password didn't work": accepted.
use impl_implicit::lock::Lock;
use spec::lock::{Config, Effect, Event, LockNote, Machine, PamOutcome, Refusal};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    for event in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit] {
        m.handle(event, &mut out);
    }
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::Refused(Refusal::AccountLocked) }, &mut out);
    assert!(out.contains(&Effect::ShowNote(LockNote::WrongPassword)));
    assert_eq!(m.error, Some(LockNote::WrongPassword));
}
