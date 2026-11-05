// P5 hover-outside: the generated `State::Outside` has no hover (its
// constructors are private to the impl; the variant literal shows the shape).
use impl_statig::pointer::{Hover, State};

fn main() {
    let _ = State::Outside { hover: Hover::default() };
}
