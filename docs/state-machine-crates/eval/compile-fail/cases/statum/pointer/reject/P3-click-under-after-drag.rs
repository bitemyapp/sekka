// P3 click-under-after-drag: once the edge press became a drag, its payload
// (`EdgeDraggingData`) keeps no `beneath`.
use impl_statum::pointer::{Outside, Pressing, Router};
use spec::pointer::{Button, Config, Effect, Hit, Point};
use statum::Branch;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let hovering = Router::<Outside>::new(Config::default()).enter();
    let Pressing::Edge(edge) = hovering.press(Point::new(0, 0), Button::Left, 1, Hit::edge(9, 1), &mut out) else {
        return;
    };
    let Branch::Second(dragging) = edge.moved(Point::new(10, 0), Hit::edge(9, 1), &mut out) else { return };
    let _ = dragging.state_data.beneath;
}
