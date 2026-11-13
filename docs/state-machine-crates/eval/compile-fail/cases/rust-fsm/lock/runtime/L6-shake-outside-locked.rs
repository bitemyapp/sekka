// L6 shake-outside-locked. Driven through `consume`, only rows into
// `Locked` (from `Checking`) shake, and only `Locked`/`Checking` have
// `ShakeTick` rows; leaving the lock screen cancels the shake: refused at
// run time.
use impl_rust_fsm::lock::screen::{Input, StateMachine};
use spec::lock::{Config, Effect, Event, LockPhase, Machine, PamOutcome};

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    use Input::*;
    for path in [&[][..], &[Lock][..]] {
        let mut t = reach(path);
        assert!(matches!(t.state(), LockPhase::Unlocked | LockPhase::Locking));
        assert!(t.consume(&ShakeTick).is_err());
    }
    let mut m = impl_rust_fsm::lock::Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Lock, &mut out);
    m.handle(Event::LockConfirmed, &mut out);
    m.handle(Event::Typed { empty: false }, &mut out);
    m.handle(Event::Submit, &mut out);
    m.handle(Event::PamDone { check: 1, outcome: PamOutcome::WrongPassword }, &mut out);
    m.handle(Event::UnlockRequested, &mut out);
    assert!(out.contains(&Effect::CancelShake));
    out.clear();
    m.handle(Event::ShakeTick, &mut out);
    assert!(out.is_empty());
}
