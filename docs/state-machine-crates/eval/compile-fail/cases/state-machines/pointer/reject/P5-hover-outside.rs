// P5 hover-outside: the hover is `Inside`'s data; its setter
// (`with_inside_data`) and accessors exist only on `Inside`'s leaves, not
// on `Router<Outside>` (and `Hover`'s fields are private).
use impl_state_machines::pointer::{Ctx, Motion, Router};
use spec::pointer::{Config, Hit, Point};

fn main() {
    let hovering = Router::new(Ctx::new(Config::default()))
        .moved(Motion { at: Point::new(0, 0), hit: Hit::on(1) })
        .unwrap();
    let hover = *hovering.inside_data().unwrap();
    let outside = Router::new(Ctx::new(Config::default()));
    let _ = outside.with_inside_data(hover);
}
