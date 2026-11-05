// W1 pending-off-the-edge. `State::Pending` carries no column, but only
// `handle` sets it (printing into the last column), and every cursor move
// off that column settles it: refused at run time.
use impl_enum::wrap::{State, Wrap};
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config { cols: 3, rows: 2 });
    let mut out = Vec::new();
    for _ in 0..3 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    assert_eq!(*m.typed(), State::Pending);
    m.handle(Event::CursorTo { row: 0, col: 0 }, &mut out);
    assert_eq!(m.state(), StateName::Normal);
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, vec![Effect::Put { row: 0, col: 0, wide: false }]);
}
