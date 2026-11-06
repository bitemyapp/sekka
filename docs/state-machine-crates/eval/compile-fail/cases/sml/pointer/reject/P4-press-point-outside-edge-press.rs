// P4 press-point-outside-edge-press: `Pressed`'s payload is the owner (a
// `Node`); only `DragsOnlyPressed` holds an `EdgePress` with the press point
// and `beneath`.
use impl_sml::pointer::{EdgePress, PointerInsideStates};
use spec::pointer::{Node, Point};

fn main() {
    let press = EdgePress { edge: Node(1), at: Point::new(0, 0), clicks: 1, beneath: None };
    let _ = PointerInsideStates::Pressed(press);
}
