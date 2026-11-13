// W4 print-while-pending. `print` matches `PendingWrap`, settles it and
// soft-wraps before writing: the cell goes to the next row (refused at run
// time; writing is an effect, outside the graph).
use impl_fsmentry::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    m.handle(Event::CursorTo { row: 0, col: 3 }, &mut out);
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(m.state(), StateName::PendingWrap);
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, vec![Effect::SoftWrap { row: 0 }, Effect::Put { row: 1, col: 0, wide: false }]);
}
