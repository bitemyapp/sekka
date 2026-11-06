// K5 retry-timer-outside-wait: act on the retry timer while `Starting`.
// Only `RetryWait` has `retry`; the only edges into `RetryWait` are
// `back_off`, whose handler arms the timer.
use impl_fsmentry::polkit::{AgentEntry, PhaseEntry, Polkit};
use spec::polkit::{Config, Cookie, Event, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    if let AgentEntry::Active(mut active) = m.entry()
        && let PhaseEntry::Starting(starting) = active.as_mut().phase_entry()
    {
        starting.retry(2);
    }
}
