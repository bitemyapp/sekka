// P4 press-point-outside-edge-press. `press_under` is a field: readable
// (and stale) in any state.
use impl_implicit::pointer::Pointer;
use spec::pointer::{Button, Config, Event, Hit, Machine, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(1) }, &mut out);
    assert_eq!(m.state(), StateName::Pressed);
    let _point = m.press_under.map(|(_, at, _, _)| at);
}
