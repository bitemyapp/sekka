// T4 place-without-answer: place over the conflicting target while the
// sheet is up. Every way out of `AskingFiles` is an answer (`skip`,
// `cancel`, `keep_both`, `replace`); none places at the target.
use impl_typestate::transfer::{AskingFiles, Placing};
use spec::transfer::{Effect, Name};

fn place_over(asking: AskingFiles, out: &mut Vec<Effect>) -> Placing {
    asking.place_at(Name::Target, out)
}

fn main() {}
