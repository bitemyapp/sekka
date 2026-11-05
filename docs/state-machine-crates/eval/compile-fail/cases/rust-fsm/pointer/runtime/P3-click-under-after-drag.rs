// P3 click-under-after-drag. The table maps a release in
// `DragsOnlyDragging` to `Up` (the edge's release only), not to `Click`:
// refused at run time. `beneath` itself stays readable beside the table.
use impl_rust_fsm::pointer::Pointer;
use impl_rust_fsm::pointer::fsm::{Input, Output, StateMachine};
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut t = StateMachine::new();
    t.consume(&Input::PressEdge).expect("a row");
    t.consume(&Input::MoveFar).expect("a row");
    assert_eq!(*t.state(), StateName::DragsOnlyDragging);
    assert_eq!(t.consume(&Input::Release).ok(), Some(Some(Output::Up)));

    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    let hit = Hit::edge(2, 3);
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit }, &mut out);
    m.handle(Event::Move { at: Point::new(9, 0), hit }, &mut out);
    assert_eq!(m.state(), StateName::DragsOnlyDragging);
    assert_eq!(m.data().beneath, Some(Node(3)));
    out.clear();
    m.handle(Event::Release { at: Point::new(9, 0), button: Button::Left, hit }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Down { node: Node(3), .. } | Effect::Up { node: Node(3), .. })));
}
