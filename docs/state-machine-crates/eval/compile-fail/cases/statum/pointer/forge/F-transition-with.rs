// Forge, P5 hover-outside. statum implements the public
// `statum::CanTransitionWith<OutsideTok>` for every state with a declared
// edge to `Outside` (here `leave`), and taking it skips the method body.
// Outside code can't make an `OutsideTok`, but `state_data` is always a
// public field, so it can move one out of another machine that is
// legitimately `Outside`. The hover is never cleared: node 1 got `HoverOn` and
// no `HoverOff`, and the machine is `Outside` with it still hovered. (The
// other public means: `transition_map` and `CanTransitionMap`, with a
// closure returning such a payload.)
use impl_statum::pointer::{Outside, Router};
use spec::pointer::{Config, Effect, Hit, Node, Point};
use statum::CanTransitionWith;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let hovering = Router::<Outside>::new(Config::default()).enter().moved(Point::new(0, 0), Hit::on(1), &mut out);
    let token = Router::<Outside>::new(Config::default()).state_data;
    let _: Router<Outside> = hovering.transition_with_data(token);
    assert_eq!(out, [Effect::HoverOn(Node(1)), Effect::Move { node: Node(1), at: Point::new(0, 0) }]);
}
