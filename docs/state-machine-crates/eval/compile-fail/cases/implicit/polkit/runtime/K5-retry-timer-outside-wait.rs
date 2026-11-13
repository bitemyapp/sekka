// K5 retry-timer-outside-wait. `RetryTimer` acts only if it takes
// `retry_armed`, the flag `state()` reads first for `RetryWait`: while
// starting it is dropped (refused at run time).
use impl_implicit::polkit::Polkit;
use spec::polkit::{Config, Cookie, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    out.clear();
    m.handle(Event::RetryTimer, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Starting);
    assert_eq!(m.generation, 1);
}
