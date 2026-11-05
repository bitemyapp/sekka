// P3 click-under-after-drag. The release transition out of
// `DragsOnlyDragging` gets only its own state (`edge`) as `from`, not
// `DragsOnlyPressed`'s `beneath`: it delivers the edge's `Up` and no click
// under (refused at run time; `beneath` itself is still stored, see P4).
use impl_finny::pointer::Pointer;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::edge(2, 7) }, &mut out);
    assert_eq!(m.state(), StateName::DragsOnlyPressed);
    m.handle(Event::Move { at: Point::new(20, 0), hit: Hit::edge(2, 7) }, &mut out);
    assert_eq!(m.state(), StateName::DragsOnlyDragging);
    out.clear();
    m.handle(Event::Release { at: Point::new(20, 0), button: Button::Left, hit: Hit::edge(2, 7) }, &mut out);
    assert!(out.contains(&Effect::Up { node: Node(2), at: Point::new(20, 0), button: Button::Left, inside: true }));
    assert!(!out.iter().any(|e| matches!(e, Effect::Down { .. } | Effect::Up { node: Node(7), .. })));
}
