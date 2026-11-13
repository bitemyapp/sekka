// K2 stale-conversation. The table has no conversation numbers (its
// `Prompt` input is any prompt); `handle`'s input function drops an event
// whose `conv` isn't the newest before it reaches the table: refused at
// run time.
use impl_rust_fsm::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 2, preferred: 0, screen: true }, &mut out);
    m.handle(Event::ChooseIdentity(1), &mut out);
    assert!(out.contains(&Effect::StartHelper { conv: 2, identity: 1 }));
    out.clear();
    m.handle(Event::Prompt { conv: 1, echo: false }, &mut out);
    m.handle(Event::Failure { conv: 1 }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Starting);
}
