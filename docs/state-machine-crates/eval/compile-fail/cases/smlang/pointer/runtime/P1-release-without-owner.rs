// P1 release-without-owner. The `Outside + Release` and `Hovering(Hover) +
// Release` rows run `enter_release` and `track_release`, which deliver no
// `Up` (those states carry no owner to name): refused at run time. Nothing
// in the types stops such an action from pushing an `Up` to the hit node.
use impl_smlang::pointer::{Events, Pointer, States};
use spec::pointer::{Button, Config, Effect, Hit, Machine, Point};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::Release((Point::new(0, 0), Button::Left, Hit::on(1)))).unwrap();
    sm.process_event(&mut out, Events::Move((Point::new(0, 0), Hit::NOTHING))).unwrap();
    assert!(matches!(sm.state_enum(), States::Hovering(_)));
    sm.process_event(&mut out, Events::Release((Point::new(0, 0), Button::Left, Hit::on(1)))).unwrap();
    assert!(!out.iter().any(|e| matches!(e, Effect::Up { .. })));
}
