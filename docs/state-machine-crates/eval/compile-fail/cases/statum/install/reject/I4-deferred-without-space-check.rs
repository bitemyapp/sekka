// I4 deferred-without-space-check: take the unchecked evaluation as the
// deferred route's `Prepared`. The only typed path from `Evaluation` to a
// `Prepared` is `evaluated`, which stands for the Evaluated event and checks
// the room for the unpacked downloads before building it; the builder is
// sealed and the payload's fields are private.
use impl_statum::install::{Evaluation, Helper, Route};

fn skip_the_check(evaluation: Helper<Evaluation>) -> Route {
    Route::Deferred(evaluation)
}

fn main() {}
