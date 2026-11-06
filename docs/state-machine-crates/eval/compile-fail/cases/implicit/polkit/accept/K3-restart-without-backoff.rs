// K3 restart-without-backoff. BUG(SYB-09, SHB-03): the shell's `Failed`
// handler restarts at once whatever the failure; an unreachable helper is
// started again straight away, no `RetryWait`: accepted.
use impl_implicit::polkit::Polkit;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true }, &mut out);
    out.clear();
    m.handle(Event::Unreachable { conv: 1 }, &mut out);
    assert!(out.contains(&Effect::StartHelper { conv: 2, identity: 0 }));
    assert!(!out.iter().any(|e| matches!(e, Effect::ArmRetry { .. })));
    assert_eq!(m.state(), StateName::Starting);
}
