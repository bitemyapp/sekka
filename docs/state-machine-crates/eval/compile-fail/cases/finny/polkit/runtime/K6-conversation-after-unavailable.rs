// K6 conversation-after-unavailable. `Unavailable` declares only
// `ChooseIdentity`: the timer and the helper events of the last
// conversation start nothing (refused at run time).
use impl_finny::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config { retry_base_ms: 500, max_unanswered: 1 });
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    m.handle(Event::Unreachable { conv: 1 }, &mut out);
    assert_eq!(m.state(), StateName::Unavailable);
    out.clear();
    for event in [Event::RetryTimer, Event::Failure { conv: 1 }, Event::Unreachable { conv: 1 }] {
        m.handle(event, &mut out);
    }
    assert!(!out.iter().any(|e| matches!(e, Effect::StartHelper { .. })));
    assert_eq!(m.state(), StateName::Unavailable);
}
