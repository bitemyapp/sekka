// K3 restart-without-backoff: an unanswered failure straight back to
// `Starting`. `Starting::failed` (and `Prompted::failed`) return a `Backoff`:
// `RetryWait` or `Unavailable`. Only `Checking::refused` restarts at once.
use impl_typestate::polkit::{Ctx, Starting};
use spec::polkit::Effect;

fn restart_at_once(ctx: &mut Ctx, starting: Starting, out: &mut Vec<Effect>) -> Starting {
    starting.failed(ctx, out)
}

fn main() {}
