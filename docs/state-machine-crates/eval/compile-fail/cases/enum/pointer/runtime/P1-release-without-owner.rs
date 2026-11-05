// P1 release-without-owner. impl-enum's `Press::None` carries no owner, and
// its `handle` delivers no `Up` for a release then: refused at run time.
use impl_enum::pointer::{Pointer, Press, State};
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Point};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    // A press on inert background: nothing owns it.
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::NOTHING }, &mut out);
    assert!(matches!(m.typed(), State::Inside { press: Press::None, .. }));
    m.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Up { .. })));
}
