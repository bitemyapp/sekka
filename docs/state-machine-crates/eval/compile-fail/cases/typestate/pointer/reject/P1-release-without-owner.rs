// P1 release-without-owner: deliver an Up while no press is owned.
// In impl-typestate only the owner states have a `release` that emits `Up`;
// `Hovering::release` takes no position or button and delivers nothing.
use impl_typestate::pointer::Outside;
use spec::pointer::{Button, Effect, Hit, Point};

fn main() {
    let hovering = Outside::new().enter();
    let mut out: Vec<Effect> = Vec::new();
    let _ = hovering.release(Point::new(0, 0), Button::Left, Hit::on(1), &mut out);
}
