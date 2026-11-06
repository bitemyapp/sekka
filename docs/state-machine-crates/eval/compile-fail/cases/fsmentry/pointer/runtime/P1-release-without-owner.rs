// P1 release-without-owner. fsmentry has no effects: an `Up` is pushed by
// `handle`, which takes the node from the data an owner state hands back
// when it's left (`Pressed::release() -> Node`). `Hovering` has nothing to
// hand back, and a release there pushes no `Up`: refused at run time, by the
// handler (nothing in the generated types stops an arm from pushing one).
use impl_fsmentry::pointer::Pointer;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::NOTHING }, &mut out);
    m.handle(Event::Move { at: Point::new(5, 0), hit: Hit::on(1) }, &mut out);
    assert_eq!(m.state(), StateName::Hovering);
    m.handle(Event::Release { at: Point::new(5, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Up { .. })));
}
