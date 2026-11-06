// K3 restart-without-backoff. `Starting`'s failure transitions go to
// `RetryWait` or `Unavailable`, whose entry shows the note (and arms the
// timer); only entering `Starting` starts a helper (refused at run time).
use impl_finny::polkit::Polkit;
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
