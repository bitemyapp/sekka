// P2 owner-after-native-drag. After a program-started drag `active` still
// names the drag source (BUG KIT-13): accepted.
use impl_implicit::pointer::Pointer;
use spec::pointer::{Button, Config, DragFrom, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(5) }, &mut out);
    m.handle(Event::StartDrag(DragFrom::Program), &mut out);
    assert_eq!(m.state(), StateName::NativeDrag);
    assert_eq!(m.active, Some(Node(5)));
}
