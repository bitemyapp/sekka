// K6 conversation-after-unavailable. With `max_unanswered` failures in a
// row the spec gives up (`Unavailable`); the shell's Failed handler restarts
// the conversation at once every time (BUG SYB-09, SHB-03), so a fourth
// conversation starts with nobody choosing an identity: accepted.
use impl_implicit::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    for conv in 1..=3 {
        m.handle(Event::Unreachable { conv }, &mut out);
    }
    assert!(out.contains(&Effect::StartHelper { conv: 4, identity: 0 }));
}
