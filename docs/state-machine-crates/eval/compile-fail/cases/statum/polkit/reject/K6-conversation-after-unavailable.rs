// K6 conversation-after-unavailable: start again from `Unavailable` on the
// timer's path, without the person choosing. `Session<Unavailable>` offers
// only `choose` and `end`.
use impl_statum::polkit::{Session, Unavailable};
use spec::polkit::Effect;

fn start_again(unavailable: Session<Unavailable>, out: &mut Vec<Effect>) {
    let _ = unavailable.retry(out);
}

fn main() {}
