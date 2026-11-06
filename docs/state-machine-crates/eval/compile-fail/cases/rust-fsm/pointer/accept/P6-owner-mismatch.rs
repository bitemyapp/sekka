// P6 owner-mismatch. The table's owner states carry no owner, and the data
// beside it isn't tied to them: through `handle`, a capture during a
// drags-only press leaves `Pressed` with an ordinary owner and the old
// `beneath` still set (read through `data()`): accepted.
use impl_rust_fsm::pointer::Pointer;
use spec::pointer::{Button, Config, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::edge(2, 3) }, &mut out);
    m.handle(Event::Capture(Node(5)), &mut out);
    assert_eq!(m.state(), StateName::Pressed);
    assert_eq!(m.data().owner, Node(5));
    assert_eq!(m.data().beneath, Some(Node(3)));
}
