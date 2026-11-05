// K6 conversation-after-unavailable: from `Unavailable` only
// `choose_identity` starts a conversation; the typed `Auth<Unavailable>`
// has no `retry_timer`.
use impl_state_machines::polkit::{Auth, AuthIdleBeginOutcome, AuthStartingUnreachableOutcome, Begin, Ctx};
use spec::polkit::{Config, Cookie};

fn main() {
    let idle = Auth::new(Ctx::new(Config { retry_base_ms: 500, max_unanswered: 1 }));
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    let AuthIdleBeginOutcome::Starting(starting) = idle.begin(begin).unwrap() else { return };
    let AuthStartingUnreachableOutcome::Unavailable(unavailable) = starting.unreachable(1).unwrap() else { return };
    let _ = unavailable.retry_timer();
}
