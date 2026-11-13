// P3 click-under-after-drag: reach the node beneath once the edge press became a drag.
use impl_typestate::pointer::{EdgeMoved, Outside, Pressing};
use spec::pointer::{Button, Effect, Hit, Point};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Pressing::Edge(edge) = Outside::new().enter().press(Point::new(0, 0), Button::Left, 1, Hit::edge(9, 1), &mut out) else {
        return;
    };
    let EdgeMoved::Drag(dragging) = edge.moved(Point::new(10, 0), Hit::edge(9, 1), 16, &mut out) else { return };
    let _ = dragging.beneath();
}
