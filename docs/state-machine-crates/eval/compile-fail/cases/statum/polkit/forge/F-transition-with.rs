// Forge, K3 restart-without-backoff (the SYB-09/SHB-03 spin). `failed`
// declares the edge `Starting → RetryWait`, so statum implements the public
// `CanTransitionWith<RetryWaitData>` for `Session<Starting>`, and taking it
// skips the back-off: no count, no `ArmRetry`. Outside code can't make a
// `RetryWaitData` (private fields), but `state_data` is always a public
// field: it moves one out of another session that legitimately failed. Then
// `retry` starts the next helper at once, every time, and `Unavailable` is
// never reached. (The other public means: `transition_map` and
// `CanTransitionMap` with a closure returning such a payload; K1 the same
// way, with a `PromptedData` from a session that was prompted.)
use impl_statum::polkit::{Idle, RetryWait, Session};
use spec::polkit::{Config, Cookie, Effect};
use statum::{Branch, CanTransitionWith};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Ok(mut starting) = Session::<Idle>::new(Config::default()).begin(Cookie(7), 1, 0, true, &mut out) else {
        panic!("no session");
    };
    for _ in 0..10 {
        // The helper failed before any answer; a payload from a donor session.
        let mut donor_out: Vec<Effect> = Vec::new();
        let Ok(donor) = Session::<Idle>::new(Config::default()).begin(Cookie(8), 1, 0, true, &mut donor_out) else {
            panic!("no donor");
        };
        let Branch::First(donor) = donor.unreachable(&mut donor_out) else { panic!("donor gave up") };
        let wait: Session<RetryWait> = starting.transition_with_data(donor.state_data);
        starting = wait.retry(&mut out);
    }
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::StartHelper { .. })).count(), 11);
    assert!(!out.iter().any(|e| matches!(e, Effect::ArmRetry { .. } | Effect::ShowNote(_))));
}
