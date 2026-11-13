// P5 hover-outside. The hover pair is `Inside`'s context: `Inside`'s exit
// clears it, and no `Router` row while `Outside` or in `NativeDrag` writes
// it (`Inside`'s own events are `Err(NoTransition)` there). Re-entering over
// nothing unhovers nothing (refused at run time; the pair itself exists
// while `Outside`, finny keeps the sub-machine's context).
use impl_finny::finny::FsmError;
use impl_finny::pointer::{Capture, InsideEvents, Pointer};
use spec::pointer::{Config, DragFrom, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Move { at: Point::new(0, 0), hit: Hit::on(3) }, &mut out);
    m.handle(Event::Leave, &mut out);
    assert_eq!(m.state(), StateName::Outside);
    for event in [Event::Capture(Node(3)), Event::Removed(Node(4)), Event::StartDrag(DragFrom::Widget), Event::DragEnded] {
        m.handle(event, &mut out);
    }
    assert_eq!(m.state(), StateName::Outside);
    assert_eq!(m.dispatch(InsideEvents::from(Capture(Node(3))), &mut out), Err(FsmError::NoTransition));
    out.clear();
    m.handle(Event::Move { at: Point::new(0, 0), hit: Hit::NOTHING }, &mut out);
    assert!(out.is_empty());
    assert_eq!(m.state(), StateName::Hovering);
}
