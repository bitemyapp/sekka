// P6 owner-mismatch: an owner state without an owner. `Pressed` always
// carries one: its owner is a `Node`, not an `Option`.
use impl_typestate::pointer::Pressed;
use spec::pointer::Node;

fn ownerless(pressed: &Pressed) -> Option<Node> {
    pressed.owner()
}

fn main() {}
