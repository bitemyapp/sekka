// K4 double-reply: end the request twice. The cookie to reply with comes
// back from `Active::end`, which consumes the handle.
use impl_fsmentry::polkit::{AgentEntry, Polkit};
use spec::polkit::{Config, Cookie, Event, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    if let AgentEntry::Active(active) = m.entry() {
        let _first = active.end();
        let _again = active.end();
    }
}
