// L5 refusal-as-wrong-password. The `refused` action maps `Refused(r)` to
// `LockNote::Refused(r)` (refused at run time: the note is computed in the
// action, nothing in the types ties it to the outcome).
use impl_smlang::lock::{Lock, LockEvents};
use spec::lock::{Config, Effect, LockNote, Machine, PamOutcome, Refusal};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, LockEvents::Lock).unwrap();
    sm.process_event(&mut out, LockEvents::LockConfirmed).unwrap();
    sm.process_event(&mut out, LockEvents::Typed(false)).unwrap();
    sm.process_event(&mut out, LockEvents::Submit).unwrap();
    sm.process_event(&mut out, LockEvents::PamDone((1, PamOutcome::Refused(Refusal::AccountLocked)))).unwrap();
    assert!(out.contains(&Effect::ShowNote(LockNote::Refused(Refusal::AccountLocked))));
    assert!(!out.contains(&Effect::ShowNote(LockNote::WrongPassword)));
}
