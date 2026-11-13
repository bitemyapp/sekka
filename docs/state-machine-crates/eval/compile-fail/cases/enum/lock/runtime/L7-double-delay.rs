// L7 double-delay. `take_delay` acts only from `Delay::NotHeld`: a second
// session start, logind reappearing or a resume while held take nothing
// (refused at run time).
use impl_enum::lock::Lock;
use spec::lock::{Config, DelayPhase, Effect, Event, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    for event in [Event::SessionStarted, Event::SessionStarted, Event::LogindAppeared, Event::Resumed] {
        m.handle(event, &mut out);
    }
    assert_eq!(out, vec![Effect::TakeDelay]);
    assert_eq!(m.state().delay, DelayPhase::Held);
}
