// P2 owner-after-native-drag. The table's states carry no data; the owner
// is a field beside the table that every state has. After `StartDrag` the
// machine is in `NativeDrag` and `data().owner` still names the node that
// had the press: readable (and stale). No row routes to it, but nothing
// ties the field to the owner states: accepted.
use impl_rust_fsm::pointer::Pointer;
use spec::pointer::{Button, Config, DragFrom, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(4) }, &mut out);
    assert_eq!(m.state(), StateName::Pressed);
    m.handle(Event::StartDrag(DragFrom::Program), &mut out);
    assert_eq!(m.state(), StateName::NativeDrag);
    assert_eq!(m.data().owner, Node(4));
}
