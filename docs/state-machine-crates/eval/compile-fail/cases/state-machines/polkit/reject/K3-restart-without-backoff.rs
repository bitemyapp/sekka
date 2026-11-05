// K3 restart-without-backoff: a failure from `Starting` (no answer given)
// has only `RetryWait` and `Unavailable` among its typed outcomes.
use impl_state_machines::polkit::{Auth, AuthIdleBeginOutcome, AuthStartingFailureOutcome, Begin, Ctx};
use spec::polkit::{Config, Cookie};

fn main() {
    let idle = Auth::new(Ctx::new(Config::default()));
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    let AuthIdleBeginOutcome::Starting(starting) = idle.begin(begin).unwrap() else { return };
    match starting.failure(1).unwrap() {
        AuthStartingFailureOutcome::Starting(_restarted) => {}
        _ => {}
    }
}
