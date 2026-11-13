// K1 answer-without-prompt: write an answer before the helper prompted. Only
// `Session<Prompted>::answer` writes one, and a `Session<Prompted>` comes only
// from `prompt` (the Prompt event); `Starting`, `Checking`, `RetryWait`,
// `Unavailable` and `Idle` have no `answer`.
use impl_statum::polkit::{Prompted, Session, Starting};
use spec::polkit::Effect;

fn answer_early(starting: Session<Starting>, out: &mut Vec<Effect>) {
    let _ = Session::<Prompted>::answer(starting, out);
}

fn main() {}
