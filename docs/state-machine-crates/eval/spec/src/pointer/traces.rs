//! Conformance traces for `pointer`. Positions are window pixels; hits are
//! given explicitly (`Hit::on(n)`, `Hit::edge(edge, under)`).

use super::{Button, Config, DragFrom, Effect as F, Event as E, Hit, Node, Point, StateName as S};
use crate::testing::{Driven, Tape, Target, Trace};

/// The `pointer` target.
pub struct Spec;

impl<M: super::Machine> Driven<Spec> for M {
    fn boot(config: Config) -> Self {
        M::new(config)
    }
    fn step(&mut self, event: E, out: &mut Vec<F>) {
        self.handle(event, out);
    }
    fn observe(&self) -> S {
        self.state()
    }
}

impl Target for Spec {
    const NAME: &'static str = "pointer";
    type Config = Config;
    type Event = E;
    type Effect = F;
    type State = S;
    type Env = ();

    fn traces() -> Vec<Trace<Self>> {
        traces()
    }
    fn configs() -> Vec<Config> {
        vec![Config::default(), Config { drag_threshold: 1 }, Config { drag_threshold: 8 }]
    }
    fn env(_: &Config) {}
    fn next(_: &mut (), tape: &mut Tape, _: S, _: &[F]) -> E {
        super::env::event(tape)
    }
}

const fn n(id: u16) -> Node {
    Node(id)
}
const fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}
fn mv(x: i32, y: i32, hit: Hit) -> E {
    E::Move { at: p(x, y), hit }
}
fn press(x: i32, y: i32, hit: Hit) -> E {
    E::Press { at: p(x, y), button: Button::Left, clicks: 1, hit }
}
fn press_b(x: i32, y: i32, button: Button, hit: Hit) -> E {
    E::Press { at: p(x, y), button, clicks: 1, hit }
}
fn release(x: i32, y: i32, hit: Hit) -> E {
    E::Release { at: p(x, y), button: Button::Left, hit }
}
fn release_b(x: i32, y: i32, button: Button, hit: Hit) -> E {
    E::Release { at: p(x, y), button, hit }
}
fn moved(id: u16, x: i32, y: i32) -> F {
    F::Move { node: n(id), at: p(x, y) }
}
fn down(id: u16, x: i32, y: i32) -> F {
    F::Down { node: n(id), at: p(x, y), button: Button::Left, clicks: 1 }
}
fn down_b(id: u16, x: i32, y: i32, button: Button) -> F {
    F::Down { node: n(id), at: p(x, y), button, clicks: 1 }
}
fn up(id: u16, x: i32, y: i32, inside: bool) -> F {
    F::Up { node: n(id), at: p(x, y), button: Button::Left, inside }
}
fn up_b(id: u16, x: i32, y: i32, button: Button, inside: bool) -> F {
    F::Up { node: n(id), at: p(x, y), button, inside }
}
fn on(id: u16) -> F {
    F::HoverOn(n(id))
}
fn off(id: u16) -> F {
    F::HoverOff(n(id))
}

pub fn traces() -> Vec<Trace<Spec>> {
    let c = Config::default();
    let edge = Hit::edge(50, 104);
    vec![
        Trace::new("hover-moves-between-nodes", "hover follows the hit test; leaving clears it", c)
            .step(mv(10, 10, Hit::on(1)), [on(1), moved(1, 10, 10)], S::Hovering)
            .step(mv(20, 10, Hit::on(1)), [moved(1, 20, 10)], S::Hovering)
            .step(mv(30, 10, Hit::on(2)), [off(1), on(2), moved(2, 30, 10)], S::Hovering)
            .step(mv(40, 10, Hit::NOTHING), [off(2)], S::Hovering)
            .step(E::Leave, [], S::Outside)
            .step(mv(50, 10, Hit::on(3)), [on(3), moved(3, 50, 10)], S::Hovering)
            .step(E::Leave, [off(3)], S::Outside)
            .step(E::Leave, [], S::Outside),
        Trace::new("click-pairs-press-and-release", "the node that took the press gets its moves and release", c)
            .step(mv(10, 10, Hit::on(1)), [on(1), moved(1, 10, 10)], S::Hovering)
            .step(press(10, 10, Hit::on(1)), [down(1, 10, 10)], S::Pressed)
            .step(mv(12, 11, Hit::on(1)), [moved(1, 12, 11)], S::Pressed)
            .step(release(12, 11, Hit::on(1)), [up(1, 12, 11, true)], S::Hovering),
        Trace::new("release-outside-the-pressed-node", "moves and the release go to the owner, inside: false", c)
            .step(press(10, 10, Hit::on(1)), [on(1), down(1, 10, 10)], S::Pressed)
            .step(mv(40, 10, Hit::on(2)), [off(1), on(2), moved(1, 40, 10)], S::Pressed)
            .step(release(40, 10, Hit::on(2)), [up(1, 40, 10, false)], S::Hovering)
            .step(mv(41, 10, Hit::on(2)), [moved(2, 41, 10)], S::Hovering),
        Trace::new(
            "drags-only-press-without-drag-clicks-under",
            "an edge press that doesn't drag is a click on the row under it",
            c,
        )
        .step(mv(300, 100, edge), [F::UnderOn(n(104)), on(50), moved(50, 300, 100)], S::Hovering)
        .step(press(300, 100, edge), [down(50, 300, 100)], S::DragsOnlyPressed)
        .step(mv(302, 101, edge), [moved(50, 302, 101)], S::DragsOnlyPressed)
        .step(
            release(302, 101, edge),
            [up(50, 302, 101, true), down(104, 300, 100), up(104, 302, 101, true)],
            S::Hovering,
        ),
        Trace::new("drags-only-press-becomes-a-drag", "moving the threshold away makes it the edge's drag", c)
            .step(mv(300, 100, edge), [F::UnderOn(n(104)), on(50), moved(50, 300, 100)], S::Hovering)
            .step(press(300, 100, edge), [down(50, 300, 100)], S::DragsOnlyPressed)
            .step(
                mv(304, 100, Hit::on(104)),
                [F::UnderOff(n(104)), off(50), on(104), moved(50, 304, 100)],
                S::DragsOnlyDragging,
            )
            .step(mv(330, 100, Hit::on(104)), [moved(50, 330, 100)], S::DragsOnlyDragging)
            .step(release(330, 100, Hit::on(104)), [up(50, 330, 100, false)], S::Hovering),
        Trace::new("other-buttons-reach-what-is-under-an-edge", "a right press on an edge goes to the row beneath", c)
            .step(mv(300, 100, edge), [F::UnderOn(n(104)), on(50), moved(50, 300, 100)], S::Hovering)
            .step(press_b(300, 100, Button::Right, edge), [down_b(104, 300, 100, Button::Right)], S::Pressed)
            .step(release_b(300, 100, Button::Right, edge), [up_b(104, 300, 100, Button::Right, true)], S::Hovering),
        Trace::new("capture-keeps-the-pointer", "a capturing node keeps the moves until it lets go", c)
            .step(press(10, 10, Hit::on(1)), [on(1), down(1, 10, 10)], S::Pressed)
            .step(E::Capture(n(1)), [], S::Pressed)
            .step(mv(40, 10, Hit::on(2)), [off(1), on(2), moved(1, 40, 10)], S::Pressed)
            .step(E::Uncapture(n(1)), [], S::Hovering)
            .step(mv(41, 10, Hit::on(2)), [moved(2, 41, 10)], S::Hovering),
        Trace::new("capture-without-a-press", "a capture alone routes the moves and the next release", c)
            .step(mv(10, 10, Hit::on(1)), [on(1), moved(1, 10, 10)], S::Hovering)
            .step(E::Capture(n(4)), [], S::Pressed)
            .step(mv(12, 10, Hit::on(1)), [moved(4, 12, 10)], S::Pressed)
            .step(release(12, 10, Hit::on(1)), [up(4, 12, 10, false)], S::Hovering),
        Trace::new("capture-moves-the-press-elsewhere", "another node's capture takes the release", c)
            .step(press(10, 10, Hit::on(1)), [on(1), down(1, 10, 10)], S::Pressed)
            .step(E::Capture(n(9)), [], S::Pressed)
            .step(release(10, 10, Hit::on(1)), [up(9, 10, 10, false)], S::Hovering),
        Trace::new("leaving-ends-the-press", "a leave while pressed means the compositor took the pointer", c)
            .step(press(10, 10, Hit::on(1)), [on(1), down(1, 10, 10)], S::Pressed)
            .step(E::Leave, [off(1)], S::Outside)
            .step(mv(12, 10, Hit::on(1)), [on(1), moved(1, 12, 10)], S::Hovering),
        Trace::new("leaving-ends-an-edge-press", "as the current code does for drags-only presses", c)
            .step(press(300, 100, edge), [F::UnderOn(n(104)), on(50), down(50, 300, 100)], S::DragsOnlyPressed)
            .step(E::Leave, [F::UnderOff(n(104)), off(50)], S::Outside)
            .step(mv(300, 100, edge), [F::UnderOn(n(104)), on(50), moved(50, 300, 100)], S::Hovering),
        Trace::new("widget-started-drag", "the compositor owns the pointer until the drag ends", c)
            .step(press(700, 250, Hit::on(205)), [on(205), down(205, 700, 250)], S::Pressed)
            .step(mv(706, 250, Hit::on(205)), [moved(205, 706, 250)], S::Pressed)
            .step(E::StartDrag(DragFrom::Widget), [off(205)], S::NativeDrag)
            .step(mv(720, 250, Hit::on(205)), [], S::NativeDrag)
            .step(release(720, 250, Hit::on(205)), [], S::NativeDrag)
            .step(E::Leave, [], S::NativeDrag)
            .step(E::DragEnded, [], S::Outside)
            .step(mv(400, 300, Hit::on(110)), [on(110), moved(110, 400, 300)], S::Hovering),
        Trace::new("second-button-while-pressed", "the first owner keeps the press; any release ends it", c)
            .step(press(10, 10, Hit::on(1)), [on(1), down(1, 10, 10)], S::Pressed)
            .step(
                press_b(40, 10, Button::Right, Hit::on(2)),
                [off(1), on(2), down_b(2, 40, 10, Button::Right)],
                S::Pressed,
            )
            .step(release_b(40, 10, Button::Right, Hit::on(2)), [up_b(1, 40, 10, Button::Right, false)], S::Hovering),
        Trace::new("second-press-cancels-the-click-under", "another button during an edge press: no click under", c)
            .step(mv(300, 100, edge), [F::UnderOn(n(104)), on(50), moved(50, 300, 100)], S::Hovering)
            .step(press(300, 100, edge), [down(50, 300, 100)], S::DragsOnlyPressed)
            .step(press_b(300, 100, Button::Right, edge), [down_b(104, 300, 100, Button::Right)], S::DragsOnlyDragging)
            .step(release(300, 100, edge), [up(50, 300, 100, true)], S::Hovering),
        Trace::new(
            "removed-node-beneath-an-edge-press",
            "the click under is dropped; hover updates after the release",
            c,
        )
        .step(press(300, 100, edge), [F::UnderOn(n(104)), on(50), down(50, 300, 100)], S::DragsOnlyPressed)
        .step(E::Removed(n(104)), [], S::DragsOnlyPressed)
        .step(release(301, 100, edge), [up(50, 301, 100, true), F::UnderOn(n(104))], S::Hovering),
        Trace::new("removed-hovered-node", "a rebuilt node is hovered afresh", c)
            .step(mv(10, 10, Hit::on(1)), [on(1), moved(1, 10, 10)], S::Hovering)
            .step(E::Removed(n(1)), [], S::Hovering)
            .step(mv(11, 10, Hit::on(1)), [on(1), moved(1, 11, 10)], S::Hovering),
        Trace::new("drag-events-outside-a-drag", "stray drag ends, and input during a drag, are ignored", c)
            .step(mv(10, 10, Hit::on(1)), [on(1), moved(1, 10, 10)], S::Hovering)
            .step(E::DragEnded, [], S::Hovering)
            .step(E::StartDrag(DragFrom::Program), [off(1)], S::NativeDrag)
            .step(E::Capture(n(1)), [], S::NativeDrag)
            .step(press(10, 10, Hit::on(1)), [], S::NativeDrag)
            .step(E::StartDrag(DragFrom::Widget), [], S::NativeDrag)
            .step(E::DragEnded, [], S::Outside)
            .step(E::Uncapture(n(1)), [], S::Outside)
            .step(mv(10, 10, Hit::on(1)), [on(1), moved(1, 10, 10)], S::Hovering),
        // Confirmed bugs, as the correct behaviour.
        Trace::new(
            "program-started-drag-releases-the-press",
            "Hako's drag out: after the drop, the next click reaches the clicked control, not the drag source",
            c,
        )
        .bug("KIT-13")
        .step(mv(700, 250, Hit::on(205)), [on(205), moved(205, 700, 250)], S::Hovering)
        .step(press(700, 250, Hit::on(205)), [down(205, 700, 250)], S::Pressed)
        .step(mv(706, 250, Hit::on(205)), [moved(205, 706, 250)], S::Pressed)
        .step(E::StartDrag(DragFrom::Program), [off(205)], S::NativeDrag)
        .step(E::Leave, [], S::NativeDrag)
        .step(E::DragEnded, [], S::Outside)
        .step(mv(20, 20, Hit::on(1)), [on(1), moved(1, 20, 20)], S::Hovering)
        .step(press(20, 20, Hit::on(1)), [down(1, 20, 20)], S::Pressed)
        .step(release(20, 20, Hit::on(1)), [up(1, 20, 20, true)], S::Hovering),
        Trace::new(
            "release-without-a-press-fires-nothing",
            "press on inert background, move onto a button, let go: nothing gets a release",
            c,
        )
        .bug("UIW-01")
        .step(mv(500, 680, Hit::NOTHING), [], S::Hovering)
        .step(press(500, 680, Hit::NOTHING), [], S::Hovering)
        .step(mv(20, 20, Hit::on(1)), [on(1), moved(1, 20, 20)], S::Hovering)
        .step(release(20, 20, Hit::on(1)), [], S::Hovering),
        Trace::new(
            "release-after-the-pressed-node-is-rebuilt-away",
            "the release doesn't land on whatever replaced the pressed node",
            c,
        )
        .bug("UIW-01")
        .step(press(20, 20, Hit::on(1)), [on(1), down(1, 20, 20)], S::Pressed)
        .step(E::Removed(n(1)), [], S::Hovering)
        .step(mv(21, 20, Hit::on(7)), [on(7), moved(7, 21, 20)], S::Hovering)
        .step(release(21, 20, Hit::on(7)), [], S::Hovering),
        Trace::new("release-after-uncapture", "a node that let go of its press doesn't get the release either", c)
            .bug("UIW-01")
            .step(press(10, 10, Hit::on(1)), [on(1), down(1, 10, 10)], S::Pressed)
            .step(E::Uncapture(n(1)), [], S::Hovering)
            .step(release(10, 10, Hit::on(1)), [], S::Hovering),
    ]
}
