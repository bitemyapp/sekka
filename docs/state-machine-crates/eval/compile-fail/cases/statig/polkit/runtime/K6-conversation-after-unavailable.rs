// K6 conversation-after-unavailable. `unavailable` handles nothing itself
// and has no conversation, so helper events find no current one, and
// `RetryTimer` needs `retry_wait`: only `ChooseIdentity` (in `active`) starts
// again (refused at run time).
use impl_statig::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config { retry_base_ms: 500, max_unanswered: 1 });
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    m.handle(Event::Unreachable { conv: 1 }, &mut out);
    assert_eq!(m.state(), StateName::Unavailable);
    out.clear();
    for event in [Event::RetryTimer, Event::Failure { conv: 1 }, Event::Unreachable { conv: 1 }, Event::Submit] {
        m.handle(event, &mut out);
    }
    assert!(!out.iter().any(|e| matches!(e, Effect::StartHelper { .. })));
    assert_eq!(m.state(), StateName::Unavailable);
    m.handle(Event::ChooseIdentity(0), &mut out);
    assert_eq!(out, vec![Effect::StartHelper { conv: 2, identity: 0 }]);
}
