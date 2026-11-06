// P6 owner-mismatch. Every typed edge into an owner state declares a
// `data:` factory, so a machine that reached one by a transition has its
// owner; the typed accessors return `Option`, so it's the factories, not
// the types, that rule out "no owner" (refused at run time). `EdgePress`
// always names its edge, so a `beneath` comes with one.
use impl_state_machines::pointer::{
    Ctx, Motion, Push, Router, RouterDragsOnlyPressedMovedOutcome, RouterOutsidePressOutcome,
};
use spec::pointer::{Button, Config, Hit, Node, Point};

fn main() {
    let push = |hit| Push { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit };
    let RouterOutsidePressOutcome::Pressed(pressed) = Router::new(Ctx::new(Config::default())).press(push(Hit::on(1))).unwrap()
    else {
        panic!()
    };
    assert_eq!(pressed.pressed_data(), Some(&Node(1)));
    let hovering = Router::new(Ctx::new(Config::default()))
        .moved(Motion { at: Point::new(0, 0), hit: Hit::on(2) })
        .unwrap();
    assert_eq!(hovering.capture(Node(7)).unwrap().pressed_data(), Some(&Node(7)));
    let edge = push(Hit::edge(9, 1));
    let RouterOutsidePressOutcome::DragsOnlyPressed(edge) = Router::new(Ctx::new(Config::default())).press(edge).unwrap()
    else {
        panic!()
    };
    assert!(edge.drags_only_pressed_data().is_some());
    let far = Motion { at: Point::new(10, 0), hit: Hit::edge(9, 1) };
    let RouterDragsOnlyPressedMovedOutcome::DragsOnlyDragging(dragging) = edge.moved(far).unwrap() else { panic!() };
    assert_eq!(dragging.drags_only_dragging_data(), Some(&Node(9)));
}
