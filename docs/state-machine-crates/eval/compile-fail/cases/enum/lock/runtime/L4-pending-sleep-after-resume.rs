// L4 pending-sleep-after-resume. `Resumed` turns `Delay::SleepPending` back
// into `Held`, so the later confirmation keeps the delay (refused at run
// time).
use impl_enum::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    m.handle(Event::Resumed, &mut out);
    assert_eq!(m.state().delay, DelayPhase::Held);
    m.handle(Event::LockConfirmed, &mut out);
    assert_eq!(m.state().lock, LockPhase::Locked);
    assert_eq!(m.state().delay, DelayPhase::Held);
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert_eq!(out.iter().filter(|e| **e == Effect::TakeDelay).count(), 1);
}
