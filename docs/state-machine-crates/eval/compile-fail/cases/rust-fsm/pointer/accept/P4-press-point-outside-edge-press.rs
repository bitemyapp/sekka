// P4 press-point-outside-edge-press. The press point and `beneath` are
// fields beside the table, present in every state: in `Pressed` (an
// ordinary press) `data().at` is readable: accepted.
use impl_rust_fsm::pointer::Pointer;
use spec::pointer::{Button, Config, Event, Hit, Machine, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Press { at: Point::new(7, 9), button: Button::Left, clicks: 1, hit: Hit::on(1) }, &mut out);
    assert_eq!(m.state(), StateName::Pressed);
    assert_eq!(m.data().at, Point::new(7, 9));
    let _beneath = m.data().beneath;
}
