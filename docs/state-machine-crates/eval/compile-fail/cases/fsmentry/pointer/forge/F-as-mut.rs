// F-as-mut (pointer): commits P1 release-without-owner. fsmentry generates
// `AsMut<Over>` on every `Inside` handle, public with the handles: swapping
// two live machines' `Over` gives a machine that never had a press another
// one's owner, and its release delivers an `Up` to node 1. (`Over` has
// private fields and isn't `Copy`, so a swap is the only write; the other
// crate-generated means are each data state's `AsMut`, and every exit
// handing back its data for another machine's edge to take.)
use impl_fsmentry::pointer::{Pointer, WindowEntry};
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

fn main() {
    let (mut a, mut b) = (Pointer::new(Config::default()), Pointer::new(Config::default()));
    let mut out = Vec::new();
    a.handle(Event::Move { at: Point::new(0, 0), hit: Hit::on(2) }, &mut out);
    b.handle(Event::Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::on(1) }, &mut out);
    assert_eq!(a.state(), StateName::Hovering);
    if let (WindowEntry::Inside(mut x), WindowEntry::Inside(mut y)) = (a.entry(), b.entry()) {
        std::mem::swap(x.as_mut(), y.as_mut());
    }
    out.clear();
    a.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(2) }, &mut out);
    assert!(out.contains(&Effect::Up { node: Node(1), at: Point::new(0, 0), button: Button::Left, inside: false }));
}
