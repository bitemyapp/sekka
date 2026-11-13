// P2 owner-after-native-drag: once the compositor took the pointer the state's
// payload is `NativeDragTok`, a token: no owner to read or route to.
use impl_statum::pointer::{Outside, Pressing, Router};
use spec::pointer::{Button, Config, Effect, Hit, Point};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let hovering = Router::<Outside>::new(Config::default()).enter();
    let Pressing::Pressed(pressed) = hovering.press(Point::new(0, 0), Button::Left, 1, Hit::on(1), &mut out) else {
        return;
    };
    let dragging = pressed.start_drag(&mut out);
    let _ = dragging.state_data.owner;
}
