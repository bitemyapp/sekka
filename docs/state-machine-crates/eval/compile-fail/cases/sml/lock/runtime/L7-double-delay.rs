// L7 double-delay. Only `NotHeld`'s rows emit `TakeDelay`: a second session
// start, logind reappearing or a resume while held find no row (`Err`).
// Refused at run time.
use impl_sml::lock::{Lock, LockEvents as E};
use spec::lock::{Config, DelayPhase, Effect, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    m.machine.process_event(&mut out, E::SessionStarted).unwrap();
    for event in [E::SessionStarted, E::LogindAppeared, E::Resumed] {
        assert!(m.machine.process_event(&mut out, event).is_err());
    }
    assert_eq!(out, [Effect::TakeDelay]);
    assert_eq!(m.state().delay, DelayPhase::Held);
}
