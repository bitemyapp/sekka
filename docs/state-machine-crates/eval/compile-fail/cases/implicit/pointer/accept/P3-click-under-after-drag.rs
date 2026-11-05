// P3 click-under-after-drag. A shape probe: implicit's state is one struct,
// so the click under's data (`press_under`, with the node beneath) is a
// field in every state, including once the edge press became a drag:
// accepted.
use impl_implicit::pointer::Pointer;
use spec::pointer::{Button, Config, Event, Hit, Machine, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::edge(9, 1) }, &mut out);
    m.handle(Event::Move { at: Point::new(20, 0), hit: Hit::edge(9, 1) }, &mut out);
    assert_eq!(m.state(), StateName::DragsOnlyDragging);
    let _beneath = m.press_under.and_then(|(_, _, _, beneath)| beneath);
}
