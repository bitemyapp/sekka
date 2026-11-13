// K3 restart-without-backoff. From `Starting`, a current `Failure` has rows
// only to `RetryWait` and `Unavailable` (back off); the restart row
// (`try_again`) exists only from `Checking` (refused at run time).
use impl_smlang::polkit::{Events, Polkit, States};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::Begin((Cookie(7), 1, 0, true))).unwrap();
    sm.process_event(&mut out, Events::Failure(1)).unwrap();
    assert!(matches!(sm.state_enum(), States::RetryWait(_)));
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::StartHelper { .. })).count(), 1);
    assert!(out.contains(&Effect::ArmRetry { ms: 500 }));
}
