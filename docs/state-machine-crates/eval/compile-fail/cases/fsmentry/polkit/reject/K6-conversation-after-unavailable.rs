// K6 conversation-after-unavailable: start again from `Unavailable` on the
// timer's path. `Unavailable` has only `choose` (the person picking an
// identity); `retry` is `RetryWait`'s.
use impl_fsmentry::polkit::{AgentEntry, PhaseEntry, Polkit};
use spec::polkit::{Config, Cookie, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config { retry_base_ms: 500, max_unanswered: 1 });
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    m.handle(Event::Unreachable { conv: 1 }, &mut out);
    assert_eq!(m.state(), StateName::Unavailable);
    if let AgentEntry::Active(mut active) = m.entry()
        && let PhaseEntry::Unavailable(unavailable) = active.as_mut().phase_entry()
    {
        unavailable.retry(2);
    }
}
