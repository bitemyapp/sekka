// P2 owner-after-native-drag: read the press owner once the compositor took
// the pointer. `NativeDrag` is a unit node of the `Window` graph: its handle
// has no `as_ref` (and the press, inside `Inside`'s data, went with
// `Inside::start_drag`).
use impl_fsmentry::pointer::{Pointer, WindowEntry};
use spec::pointer::{Button, Config, DragFrom, Event, Hit, Machine, Node, Point};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(1) }, &mut out);
    m.handle(Event::StartDrag(DragFrom::Program), &mut out);
    if let WindowEntry::NativeDrag(drag) = m.entry() {
        let _owner: &Node = drag.as_ref();
    }
}
