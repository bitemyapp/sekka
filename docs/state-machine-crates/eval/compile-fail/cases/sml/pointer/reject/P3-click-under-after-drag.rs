// P3 click-under-after-drag: `DragsOnlyDragging`'s payload is the edge (a
// `Node`), without the press point and `beneath` a click under needs. (A
// child state's payload can't be read from outside at all; the variant
// literal shows its shape.)
use impl_sml::pointer::{EdgePress, PointerInsideStates};
use spec::pointer::{Node, Point};

fn main() {
    let press = EdgePress { edge: Node(1), at: Point::new(0, 0), clicks: 1, beneath: Some(Node(2)) };
    let _ = PointerInsideStates::DragsOnlyDragging(press);
}
