// L5 refusal-as-wrong-password. The `refuse` action maps `Refused(r)` to
// `LockNote::Refused(r)`; nothing in the types stops it from showing
// `WrongPassword`, but it doesn't. Refused at run time.
use impl_sml::lock::{Lock, LockEvents as E, PamDone};
use spec::lock::{Config, Effect, LockNote, Machine, PamOutcome, Refusal};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    for event in [E::Lock, E::LockConfirmed, E::Typed(false), E::Submit] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    let refused = PamDone { check: 1, outcome: PamOutcome::Refused(Refusal::AccountLocked) };
    m.machine.process_event(&mut out, E::PamDone(refused)).unwrap();
    assert!(out.contains(&Effect::ShowNote(LockNote::Refused(Refusal::AccountLocked))));
    assert!(!out.contains(&Effect::ShowNote(LockNote::WrongPassword)));
}
