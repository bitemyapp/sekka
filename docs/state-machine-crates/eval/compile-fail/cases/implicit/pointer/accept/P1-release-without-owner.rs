// P1 release-without-owner. With no `active` node the release still bubbles
// to whatever is under the pointer (BUG UIW-01): accepted.
use impl_implicit::pointer::Pointer;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::NOTHING }, &mut out);
    m.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    assert!(out.contains(&Effect::Up { node: Node(1), at: Point::new(0, 0), button: Button::Left, inside: true }));
}
