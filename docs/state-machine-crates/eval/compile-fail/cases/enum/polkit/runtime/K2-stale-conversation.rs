// K2 stale-conversation. `handle` compares the event's `conv` with the
// phase's: conversation 1's prompt is dropped once 2 runs (refused at run
// time).
use impl_enum::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 2, preferred: 0, screen: true }, &mut out);
    m.handle(Event::ChooseIdentity(1), &mut out);
    assert!(out.contains(&Effect::StartHelper { conv: 2, identity: 1 }));
    out.clear();
    m.handle(Event::Prompt { conv: 1, echo: false }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Starting);
}
