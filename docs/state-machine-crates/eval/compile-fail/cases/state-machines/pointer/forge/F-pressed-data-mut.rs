// Forge (P1 release-without-owner). The generated, public `pressed_data_mut`
// rewrites a typed `Router<Pressed>`'s owner; its release then delivers an
// `Up` to node 9, which never took the press. Other means that stay public:
// `with_<state>_data`, `<state>_data_mut` and the generic
// `state_data_<state>_mut` on every typed machine; `set_<state>_data` and
// `<state>_data_mut` on the wrapper `into_dynamic()` returns (its type
// isn't exported, so `new_init_state` can't be named); `initialize()` on
// every state type, which re-runs entry hooks (pointer has none).
use impl_state_machines::pointer::{Ctx, Lift, Push, Router, RouterOutsidePressOutcome};
use spec::pointer::{Button, Config, Effect, Hit, Node, Point};

fn main() {
    let push = Push { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(1) };
    let RouterOutsidePressOutcome::Pressed(mut pressed) = Router::new(Ctx::new(Config::default())).press(push).unwrap()
    else {
        panic!()
    };
    *pressed.pressed_data_mut().unwrap() = Node(9);
    pressed.take_effects();
    let lift = Lift { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) };
    let mut hovering = pressed.release(lift).unwrap();
    let up = Effect::Up { node: Node(9), at: Point::new(0, 0), button: Button::Left, inside: false };
    assert_eq!(hovering.take_effects()[0], up);
}
