// K5 retry-timer-outside-wait. Only `retry_wait` handles `RetryTimer`, and
// only its entry action arms the timer: while starting, the tick bubbles up
// and is ignored (refused at run time).
use impl_statig::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::RetryTimer, &mut out);
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    out.clear();
    m.handle(Event::RetryTimer, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Starting);
    m.handle(Event::Prompt { conv: 1, echo: false }, &mut out);
    m.handle(Event::Failure { conv: 1 }, &mut out);
    assert_eq!(m.state(), StateName::RetryWait);
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::ArmRetry { .. })).count(), 1);
}
