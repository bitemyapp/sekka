// I4 deferred-without-space-check: reach the deferred route's `Prepared`
// without the check of the root's room. The only typed path from
// `Evaluation` to a `Prepared` is `evaluated`, which stands for the
// Evaluated event and checks the room for the unpacked downloads before
// building it; there is no other transition to take.
use impl_typestate::install::{Evaluation, Prepared};
use spec::install::Effect;

fn skip_the_check(evaluation: Evaluation, out: &mut Vec<Effect>) -> Prepared {
    evaluation.prepared(out)
}

fn main() {}
