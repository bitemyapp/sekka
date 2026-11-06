// P6 owner-mismatch: an owner state without an owner. `Pressed` always
// carries one: its payload's owner is a `Node`, not an `Option`.
use impl_statum::pointer::{Pressed, Router};
use spec::pointer::Node;

fn ownerless(pressed: &Router<Pressed>) -> Option<Node> {
    pressed.state_data.owner()
}

fn main() {}
