// P5 hover-outside: `Router<Outside>`'s payload is `OutsideTok`; a hover
// can't be put there (`state_data` is a public field, but its type is the
// state's).
use impl_statum::pointer::{Outside, Router};
use spec::pointer::Config;

fn main() {
    let hovering = Router::<Outside>::new(Config::default()).enter();
    let mut outside = Router::<Outside>::new(Config::default());
    outside.state_data = hovering.state_data;
}
