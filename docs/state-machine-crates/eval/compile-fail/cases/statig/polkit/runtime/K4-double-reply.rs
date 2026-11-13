// K4 double-reply. The reply is `active`'s exit action, which runs once when
// the request ends; in `idle` the request's events are dropped (refused at
// run time).
use impl_statig::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    m.handle(Event::Success { conv: 1 }, &mut out);
    m.handle(Event::Success { conv: 1 }, &mut out);
    m.handle(Event::Dismiss, &mut out);
    m.handle(Event::CancelRequest { cookie: Cookie(7) }, &mut out);
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::Reply { .. })).count(), 1);
    assert_eq!(m.state(), StateName::Idle);
}
