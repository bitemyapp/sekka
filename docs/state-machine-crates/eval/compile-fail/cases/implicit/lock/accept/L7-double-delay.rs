// L7 double-delay. BUG(SYB-12): the shell takes the delay again on
// `Resumed` even while one is held: accepted.
use impl_implicit::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    assert_eq!(m.state().delay, DelayPhase::Held);
    m.handle(Event::Resumed, &mut out);
    assert_eq!(out, vec![Effect::TakeDelay, Effect::TakeDelay]);
}
