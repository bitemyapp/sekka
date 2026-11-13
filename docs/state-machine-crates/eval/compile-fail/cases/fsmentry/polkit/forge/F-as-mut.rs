// F-as-mut (polkit): commits K2 stale-conversation. fsmentry generates
// `AsMut<u32>` on the `Starting`/`Prompted`/`Checking` handles, public with
// the handles: rewriting the running conversation to 1 while 2 runs makes
// conversation 1's prompt count. (Other crate-generated means: `AsMut` on
// `Active`'s handle, for swapping two live machines' `Session`; every exit
// handing back its data for another machine's edge to take.)
use impl_fsmentry::polkit::{AgentEntry, PhaseEntry, Polkit};
use spec::polkit::{Config, Cookie, Effect, Event, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 2, preferred: 0, screen: true }, &mut out);
    m.handle(Event::ChooseIdentity(1), &mut out);
    if let AgentEntry::Active(mut active) = m.entry()
        && let PhaseEntry::Starting(mut starting) = active.as_mut().phase_entry()
    {
        *starting.as_mut() = 1;
    }
    out.clear();
    m.handle(Event::Prompt { conv: 1, echo: false }, &mut out);
    assert_eq!(out, vec![Effect::ShowPrompt { echo: false }]);
}
