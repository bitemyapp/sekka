// P2 owner-after-native-drag: read the press owner once the compositor took the pointer.
use impl_typestate::pointer::{Outside, Pressing};
use spec::pointer::{Button, Effect, Hit, Point};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Pressing::Pressed(pressed) = Outside::new().enter().press(Point::new(0, 0), Button::Left, 1, Hit::on(1), &mut out) else {
        return;
    };
    let dragging = pressed.start_drag(&mut out);
    let _ = dragging.owner();
}
