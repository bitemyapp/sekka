// K1 answer-without-prompt: submit an answer while the conversation is
// `Starting`. In the `Phase` graph only `Prompted` has `submit`.
use impl_fsmentry::polkit::{AgentEntry, PhaseEntry, Polkit};
use spec::polkit::{Config, Cookie, Event, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    if let AgentEntry::Active(mut active) = m.entry()
        && let PhaseEntry::Starting(starting) = active.as_mut().phase_entry()
    {
        starting.submit(1);
    }
}
