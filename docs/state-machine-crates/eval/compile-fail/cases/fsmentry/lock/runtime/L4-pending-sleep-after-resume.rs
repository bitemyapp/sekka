// L4 pending-sleep-after-resume. Nothing typed forces a transition when an
// event arrives: `handle` takes `resume` (`SleepPending -> Held`) on
// `Resumed`, so a confirmation after it keeps the delay (refused at run time).
use impl_fsmentry::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::PrepareForSleep, &mut out);
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    m.handle(Event::Resumed, &mut out);
    assert_eq!(m.state().delay, DelayPhase::Held);
    out.clear();
    m.handle(Event::LockConfirmed, &mut out);
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert_eq!(m.state().delay, DelayPhase::Held);
}
