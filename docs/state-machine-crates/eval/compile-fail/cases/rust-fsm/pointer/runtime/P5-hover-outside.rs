// P5 hover-outside. The hover is private data beside the table, written
// only by the actions; the table has no row that hovers in `NativeDrag`
// (a move there is `Err`, ignored), and `Outside`'s moves go to `Hovering`.
// Refused at run time; nothing in the types ties the hover to `Inside`.
use impl_rust_fsm::pointer::Pointer;
use impl_rust_fsm::pointer::fsm::{Input, StateMachine};
use spec::pointer::{Config, DragFrom, Event, Hit, Machine, Point, StateName};

fn main() {
    let mut t = StateMachine::new();
    t.consume(&Input::StartDrag).expect("a row");
    assert_eq!(*t.state(), StateName::NativeDrag);
    assert!(t.consume(&Input::Move).is_err());

    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::StartDrag(DragFrom::Widget), &mut out);
    m.handle(Event::Move { at: Point::new(1, 1), hit: Hit::on(3) }, &mut out);
    assert_eq!(m.state(), StateName::NativeDrag);
    assert!(out.is_empty());
    assert_eq!(m.data().hot, None);
}
