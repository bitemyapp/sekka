// L4 pending-sleep-after-resume. BUG(SYB-12): `Resumed` takes the delay
// again and leaves `sleep_pending` set, so the confirmation releases the
// delay just taken for the next sleep: accepted.
use impl_implicit::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    m.handle(Event::Resumed, &mut out);
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    out.clear();
    m.handle(Event::LockConfirmed, &mut out);
    assert_eq!(m.state().lock, LockPhase::Locked);
    assert!(out.contains(&Effect::ReleaseDelay));
    assert_eq!(m.state().delay, DelayPhase::NotHeld);
}
