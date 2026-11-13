// P2 owner-after-native-drag. finny keeps every state's struct for the
// machine's life: after the program starts a drag, `Inside` is stopped but
// `Pressed` still names the drag source, readable with `get_state` (and
// routable by anything that reads it): accepted.
use impl_finny::pointer::{Pointer, Pressed};
use spec::pointer::{Button, Config, DragFrom, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(5) }, &mut out);
    m.handle(Event::StartDrag(DragFrom::Program), &mut out);
    assert_eq!(m.state(), StateName::NativeDrag);
    assert_eq!(m.inside().get_state::<Pressed>().owner, Node(5));
}
