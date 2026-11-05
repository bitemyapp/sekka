// K3 restart-without-backoff. A failure in `Phase::Starting` goes through
// `back_off`: `RetryWait` with the timer armed, no new helper (refused at
// run time).
use impl_enum::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    out.clear();
    m.handle(Event::Failure { conv: 1 }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::StartHelper { .. })));
    assert_eq!(m.state(), StateName::RetryWait);
}
