// K4 double-reply: after `success` replied, the machine is an `Auth<Idle>`,
// which has no `dismiss` (nor `success`, `cancel_request`) to reply again.
use impl_state_machines::polkit::{Auth, AuthIdleBeginOutcome, Begin, Ctx};
use spec::polkit::{Config, Cookie};

fn main() {
    let idle = Auth::new(Ctx::new(Config::default()));
    let begin = Begin { cookie: Cookie(7), identities: 1, preferred: 0, screen: true };
    let AuthIdleBeginOutcome::Starting(starting) = idle.begin(begin).unwrap() else { return };
    let idle = starting.success(1).unwrap();
    let _ = idle.dismiss();
}
