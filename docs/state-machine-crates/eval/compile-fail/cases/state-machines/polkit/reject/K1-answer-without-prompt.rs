// K1 answer-without-prompt: `submit` is declared only from `Prompted`, so
// the typed `Auth<Starting>` has no such method.
use impl_state_machines::polkit::{Auth, AuthIdleBeginOutcome, Begin, Ctx};
use spec::polkit::{Config, Cookie};

fn main() {
    let idle = Auth::new(Ctx::new(Config::default()));
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    let AuthIdleBeginOutcome::Starting(starting) = idle.begin(begin).unwrap() else { return };
    let _ = starting.submit();
}
