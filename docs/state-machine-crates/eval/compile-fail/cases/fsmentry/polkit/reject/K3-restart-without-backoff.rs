// K3 restart-without-backoff: restart at once from `Starting` after an
// unanswered failure. `restart` is `Checking`'s (a failure after an
// answer); `Starting` fails into `back_off` or `give_up`, and its other
// edges are `prompt` and `choose` (the person picking an identity).
use impl_fsmentry::polkit::{AgentEntry, PhaseEntry, Polkit};
use spec::polkit::{Config, Cookie, Event, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    if let AgentEntry::Active(mut active) = m.entry()
        && let PhaseEntry::Starting(starting) = active.as_mut().phase_entry()
    {
        starting.restart(2);
    }
}
