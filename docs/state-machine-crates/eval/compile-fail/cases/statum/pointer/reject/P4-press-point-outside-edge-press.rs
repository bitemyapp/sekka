// P4 press-point-outside-edge-press: an ordinary press's payload
// (`PressedData`) has no press point.
use impl_statum::pointer::{Outside, Pressing, Router};
use spec::pointer::{Button, Config, Effect, Hit, Point};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let hovering = Router::<Outside>::new(Config::default()).enter();
    let Pressing::Pressed(pressed) = hovering.press(Point::new(0, 0), Button::Left, 1, Hit::on(1), &mut out) else {
        return;
    };
    let _ = pressed.state_data.at;
}
