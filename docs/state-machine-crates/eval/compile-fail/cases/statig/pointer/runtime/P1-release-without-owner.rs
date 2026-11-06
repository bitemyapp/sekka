// P1 release-without-owner. statig's handlers decide; `outside` and
// `hovering` deliver no `Up` for a release: refused at run time (nothing in
// the types forbids a handler from pushing one).
use impl_statig::pointer::Pointer;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Point};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    m.handle(Event::Leave, &mut out);
    m.handle(Event::Move { at: Point::new(0, 0), hit: Hit::NOTHING }, &mut out);
    m.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Up { .. })));
}
