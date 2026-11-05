// P5 hover-outside: `State::Outside` has no hover.
use impl_enum::pointer::{Hover, State};

fn main() {
    let _ = State::Outside { hover: Hover::default() };
}
