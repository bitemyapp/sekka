// K3 restart-without-backoff: an unanswered failure straight back to
// `Starting`. `Session<Starting>::failed` (and `Session<Prompted>::failed`)
// return a `Backoff`: `RetryWait` (timer armed) or `Unavailable`. Only
// `Session<Checking>::refused` (a refused answer) restarts at once.
use impl_statum::polkit::{Session, Starting};
use spec::polkit::Effect;

fn restart_at_once(starting: Session<Starting>, out: &mut Vec<Effect>) -> Session<Starting> {
    starting.failed(out)
}

fn main() {}
