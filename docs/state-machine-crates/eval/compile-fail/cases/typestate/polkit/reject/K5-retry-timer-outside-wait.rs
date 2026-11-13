// K5 retry-timer-outside-wait: act on the retry timer while a conversation
// is starting. Only `RetryWait` has `retry`.
use impl_typestate::polkit::{Ctx, Starting};
use spec::polkit::Effect;

fn on_timer(ctx: &mut Ctx, starting: Starting, out: &mut Vec<Effect>) {
    let _ = starting.retry(ctx, out);
}

fn main() {}
