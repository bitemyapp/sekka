// P1 release-without-owner: every typed transition that delivers an `Up` is
// the `release` of an owner state (`Router<Pressed>`, `Router<DragsOnly*>`),
// and only `press`, `capture` or `moved` build one; a `Router<Hovering>`
// can't take the owner's release.
use impl_state_machines::pointer::{Ctx, Lift, Motion, Pressed, Router};
use spec::pointer::{Button, Config, Hit, Point};

fn main() {
    let outside = Router::new(Ctx::new(Config::default()));
    let hovering = outside.moved(Motion { at: Point::new(0, 0), hit: Hit::NOTHING }).unwrap();
    let lift = Lift { at: Point::new(5, 5), button: Button::Left, hit: Hit::on(1) };
    let _ = Router::<Pressed>::release(hovering, lift);
}
