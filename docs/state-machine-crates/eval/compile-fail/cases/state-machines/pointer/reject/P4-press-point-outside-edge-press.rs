// P4 press-point-outside-edge-press: the press point and `beneath` are
// `DragsOnlyPressed`'s data; a `Router<Hovering>` has no accessor for it.
use impl_state_machines::pointer::{Ctx, Motion, Router};
use spec::pointer::{Config, Hit, Point};

fn main() {
    let outside = Router::new(Ctx::new(Config::default()));
    let hovering = outside.moved(Motion { at: Point::new(0, 0), hit: Hit::on(1) }).unwrap();
    let _ = hovering.drags_only_pressed_data();
}
