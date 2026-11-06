// L7 double-delay: take the delay while it's held. Only `NotHeld` has
// `take_delay`; `SleepPending -resume-> Held` takes no second delay.
use impl_fsmentry::lock::{DelayEntry, Lock};
use spec::lock::{Config, DelayPhase, Event, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SessionStarted, &mut out);
    assert_eq!(m.state().delay, DelayPhase::Held);
    if let DelayEntry::Held(held) = m.delay_entry() {
        held.take_delay();
    }
}
