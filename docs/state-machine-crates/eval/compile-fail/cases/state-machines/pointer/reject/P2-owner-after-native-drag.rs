// P2 owner-after-native-drag: the owner is `Pressed`'s data, and its typed
// accessor exists only on `Router<Pressed>`. (The generic
// `state_data_pressed()` exists on every state type and returns `None`
// here: leaving `Pressed` cleared it.)
use impl_state_machines::pointer::{Ctx, Push, Router, RouterOutsidePressOutcome};
use spec::pointer::{Button, Config, Hit, Point};

fn main() {
    let outside = Router::new(Ctx::new(Config::default()));
    let press = Push { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(1) };
    let RouterOutsidePressOutcome::Pressed(pressed) = outside.press(press).unwrap() else { return };
    let native = pressed.start_drag().unwrap();
    let _ = native.pressed_data();
}
