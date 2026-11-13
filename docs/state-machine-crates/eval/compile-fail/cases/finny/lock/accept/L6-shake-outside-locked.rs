// L6 shake-outside-locked. The shake counter (the timer runs while it's
// > 0) is `Locked`'s context, which finny keeps for the machine's life:
// unlocked mid-shake, `Locked`'s exit cancels the timer (`CancelShake`) but
// its data still says the shake runs, read through the state accessor while
// `Unlocked` and while locking again (it is reset only on entering
// `Locked`): accepted.
use impl_finny::lock::{Lock, Locked};
use spec::lock::{Config, Effect, Event, LockPhase, Machine, PamOutcome};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    for event in [
        Event::Lock,
        Event::LockConfirmed,
        Event::Typed { empty: false },
        Event::Submit,
        Event::PamDone { check: 1, outcome: PamOutcome::WrongPassword },
    ] {
        m.handle(event, &mut out);
    }
    assert!(out.contains(&Effect::ArmShake));
    m.handle(Event::UnlockRequested, &mut out);
    assert!(out.contains(&Effect::CancelShake));
    assert_eq!(m.state().lock, LockPhase::Unlocked);
    assert_eq!(m.lockscreen().get_state::<Locked>().context.shake_step(), 1);
    m.handle(Event::Lock, &mut out);
    assert_eq!(m.state().lock, LockPhase::Locking);
    assert_eq!(m.lockscreen().get_state::<Locked>().context.shake_step(), 1);
}
