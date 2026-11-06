// K6 conversation-after-unavailable: start again from `Unavailable` on the
// timer's path, without the person choosing. `Unavailable` offers only
// `choose` and `end`.
use impl_typestate::polkit::{Ctx, Unavailable};
use spec::polkit::Effect;

fn start_again(ctx: &mut Ctx, unavailable: Unavailable, out: &mut Vec<Effect>) {
    let _ = unavailable.retry(ctx, out);
}

fn main() {}
