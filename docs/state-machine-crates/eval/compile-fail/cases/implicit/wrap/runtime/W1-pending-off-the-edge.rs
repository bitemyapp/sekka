// W1 pending-off-the-edge. `wrap_pending` is a flag beside the column, but
// only a print reaching the last column sets it, and every cursor move off
// that column clears it: refused at run time.
use impl_implicit::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config { cols: 3, rows: 2 });
    let mut out = Vec::new();
    for _ in 0..3 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    assert!(m.wrap_pending && m.col == 2);
    m.handle(Event::CursorTo { row: 0, col: 0 }, &mut out);
    assert_eq!(m.state(), StateName::Normal);
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, vec![Effect::Put { row: 0, col: 0, wide: false }]);
}
