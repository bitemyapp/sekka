// K5 retry-timer-outside-wait. Only `RetryWait` declares a `RetryTimer`
// transition: in `Starting` the timer is `Err(NoTransition)` and starts
// nothing (refused at run time; finny's own state timers aren't used).
use impl_finny::polkit::Polkit;
use spec::polkit::{Config, Cookie, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    out.clear();
    m.handle(Event::RetryTimer, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Starting);
    assert_eq!(m.active().context.last_conv(), 1);
}
