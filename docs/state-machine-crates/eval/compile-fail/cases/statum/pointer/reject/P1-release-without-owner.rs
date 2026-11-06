// P1 release-without-owner: deliver an `Up` while no press is owned. Only the
// owner states' `#[transition]` blocks have a `release` that delivers one, and
// a `Router<Hovering>` isn't a `Router<Pressed>`. (`Router<Hovering>::release`
// takes no position or button and delivers nothing.)
use impl_statum::pointer::{Outside, Pressed, Router};
use spec::pointer::{Button, Config, Effect, Hit, Point};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let hovering = Router::<Outside>::new(Config::default()).enter();
    let _ = Router::<Pressed>::release(hovering, Point::new(0, 0), Button::Left, Hit::on(1), &mut out);
}
