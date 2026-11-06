// W5 dangling-link. The links are private data beside the table; the
// only code that sets one (the print action's soft wrap) moves to the next
// row (or scrolls) in the same step. At the bottom row the link is made,
// the screen scrolls, and the print lands on the continuation: refused at
// run time (by the action's code; neither the table nor the types know
// about links).
use impl_rust_fsm::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine};

fn main() {
    let mut m = Wrap::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::CursorTo { row: 23, col: 79 }, &mut out);
    m.handle(Event::Print { wide: false }, &mut out);
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, [Effect::SoftWrap { row: 23 }, Effect::ScrollUp, Effect::Put { row: 23, col: 0, wide: false }]);
    assert_eq!(m.data().links, 1 << 22);
}
