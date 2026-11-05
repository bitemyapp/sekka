// K2 stale-conversation. The typed transitions don't take a conversation
// number: the runtime wrapper compares the event's with the running
// `Conversation` (`Conversation::is`) and drops an older one's events.
use impl_typestate::polkit::{Any, Polkit};
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
    let Any::Starting(s) = m.typed() else { panic!("not starting") };
    assert!(s.conversation().is(2));
}
