// L5 refusal-as-wrong-password. The note is an effect `handle` derives from
// the outcome; PAM's reason is passed through (refused at run time).
use impl_fsmentry::lock::Lock;
use spec::lock::{Config, Effect, Event, LockNote, Machine, PamOutcome, Refusal};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    for e in [Event::Lock, Event::LockConfirmed, Event::Typed { empty: false }, Event::Submit] {
        m.handle(e, &mut out);
    }
    out.clear();
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::Refused(Refusal::AccountLocked) }, &mut out);
    assert_eq!(out[0], Effect::ShowNote(LockNote::Refused(Refusal::AccountLocked)));
}
