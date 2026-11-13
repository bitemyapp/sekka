// K1 answer-without-prompt: write an answer before the helper prompted.
// In impl-typestate only `Prompted::answer` writes one; `Starting`,
// `Checking`, `RetryWait`, `Unavailable` and `Idle` have no `answer`.
use impl_typestate::polkit::Starting;
use spec::polkit::Effect;

fn answer_early(starting: Starting, out: &mut Vec<Effect>) {
    let _ = starting.answer(out);
}

fn main() {}
