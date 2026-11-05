// P3 click-under-after-drag: the click under needs `beneath` and the press
// point, `DragsOnlyPressed`'s data; once the press became a drag the
// machine is a `Router<DragsOnlyDragging>`, which has no accessor for it.
use impl_state_machines::pointer::{
    Ctx, Motion, Push, Router, RouterDragsOnlyPressedMovedOutcome, RouterOutsidePressOutcome,
};
use spec::pointer::{Button, Config, Hit, Point};

fn main() {
    let outside = Router::new(Ctx::new(Config::default()));
    let press = Push { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::edge(9, 1) };
    let RouterOutsidePressOutcome::DragsOnlyPressed(edge) = outside.press(press).unwrap() else { return };
    let far = Motion { at: Point::new(10, 0), hit: Hit::edge(9, 1) };
    let RouterDragsOnlyPressedMovedOutcome::DragsOnlyDragging(dragging) = edge.moved(far).unwrap() else { return };
    let _ = dragging.drags_only_pressed_data();
}
