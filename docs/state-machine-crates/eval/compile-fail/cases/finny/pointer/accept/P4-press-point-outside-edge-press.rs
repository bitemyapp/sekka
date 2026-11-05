// P4 press-point-outside-edge-press. `DragsOnlyPressed`'s struct outlives
// the state: after the click, in `Hovering`, its press point and `beneath`
// are still there through `get_state`: accepted.
use impl_finny::pointer::{DragsOnlyPressed, Pointer};
use spec::pointer::{Button, Config, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(10, 10), button: Button::Left, clicks: 1, hit: Hit::edge(2, 7) }, &mut out);
    m.handle(Event::Release { at: Point::new(10, 10), button: Button::Left, hit: Hit::edge(2, 7) }, &mut out);
    assert_eq!(m.state(), StateName::Hovering);
    let p = m.inside().get_state::<DragsOnlyPressed>();
    assert_eq!((p.at, p.beneath), (Point::new(10, 10), Some(Node(7))));
}
