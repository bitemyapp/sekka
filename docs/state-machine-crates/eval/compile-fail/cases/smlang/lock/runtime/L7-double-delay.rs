// L7 double-delay. The delay region declares `Take` and `Resumed` with
// `take` only from `NotHeld`; while `Held` they are `Err(InvalidEvent)`
// inside `Machine::handle`, the only way to the delay region (refused at run
// time).
use impl_smlang::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::SessionStarted, &mut out);
    m.handle(Event::LogindAppeared, &mut out);
    m.handle(Event::Resumed, &mut out);
    assert_eq!(m.state().delay, DelayPhase::Held);
    assert_eq!(out.iter().filter(|e| **e == Effect::TakeDelay).count(), 1);
}
