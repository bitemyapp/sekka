// K5 retry-timer-outside-wait: act on the retry timer while a conversation is
// starting. Only `Session<RetryWait>` has `retry`.
use impl_statum::polkit::{Session, Starting};
use spec::polkit::Effect;

fn on_timer(starting: Session<Starting>, out: &mut Vec<Effect>) {
    let _ = starting.retry(out);
}

fn main() {}
