// P5 hover-outside. The hover pair is `Inside`'s data, but a composite state
// has no payload, so it lives in the private context in every state; no API
// sets it. `Inside`'s exit clears it and its entry starts it afresh, and only
// `Inside`'s rows track it: in `NativeDrag` a move or a removal is taken by
// no row (`Err`) and nothing is hovered. Refused at run time.
use impl_sml::pointer::{Move, Pointer, PointerStates, Removed, StartDrag};
use spec::pointer::{Config, Effect, Hit, Machine, Node, Point};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    sm.process_event(&mut out, Move { at: Point::new(1, 1), hit: Hit::on(3) }).unwrap();
    sm.process_event(&mut out, StartDrag).unwrap();
    assert_eq!(out.last(), Some(&Effect::HoverOff(Node(3))));
    out.clear();
    assert!(sm.process_event(&mut out, Move { at: Point::new(2, 2), hit: Hit::on(4) }).is_err());
    assert!(sm.process_event(&mut out, Removed(Node(4))).is_err());
    assert!(sm.is(&PointerStates::NativeDrag));
    assert!(out.is_empty());
}
