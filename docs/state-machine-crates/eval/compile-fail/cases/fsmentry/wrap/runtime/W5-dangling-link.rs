// W5 dangling-link. The links are private fields beside the graph; only
// `handle` reaches them, and its soft wrap links the row and moves on in one
// step. At the bottom row the link scrolls up with its row: EL on the
// continuation unlinks nothing, EL on the wrapped row unlinks it (refused at
// run time: nothing typed is involved).
use impl_fsmentry::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    m.handle(Event::CursorTo { row: 1, col: 3 }, &mut out);
    m.handle(Event::Print { wide: false }, &mut out);
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, vec![Effect::SoftWrap { row: 1 }, Effect::ScrollUp, Effect::Put { row: 1, col: 0, wide: false }]);
    out.clear();
    m.handle(Event::EraseLine(0), &mut out);
    assert!(out.is_empty());
    m.handle(Event::CursorTo { row: 0, col: 0 }, &mut out);
    m.handle(Event::EraseLine(0), &mut out);
    assert_eq!(out, vec![Effect::Unlink { row: 0 }]);
}
