// L7 double-delay: take logind's delay while one is held. Only
// `Inhibitor<NotHeld>` has `take`; a held delay is an `Inhibitor<Held>`.
use impl_statum::lock::{Held, Inhibitor, NotHeld};
use spec::lock::Effect;

fn take_again(fd: Inhibitor<Held>, out: &mut Vec<Effect>) -> Inhibitor<Held> {
    Inhibitor::<NotHeld>::take(fd, out)
}

fn main() {}
