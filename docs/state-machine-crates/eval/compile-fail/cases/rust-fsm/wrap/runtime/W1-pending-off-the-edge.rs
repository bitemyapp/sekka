// W1 pending-off-the-edge. The pending flag is the table's state; the
// column is data beside it, so the table's `Reached` row can't see the
// column (a bare table takes `Reached` in any column). Only the print
// action feeds `Reached`, when the column hits the edge, and restoring a
// saved pending cursor restores its column too: refused at run time.
use impl_rust_fsm::wrap::Wrap;
use spec::wrap::{Config, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config::default());
    let mut out = Vec::new();
    for _ in 0..80 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    assert_eq!(m.state(), StateName::PendingWrap);
    m.handle(Event::SaveCursor, &mut out);
    m.handle(Event::CursorTo { row: 3, col: 4 }, &mut out);
    m.handle(Event::Tab, &mut out);
    m.handle(Event::RestoreCursor, &mut out);
    m.handle(Event::Tab, &mut out);
    assert_eq!(m.state(), StateName::PendingWrap);
    assert_eq!(m.data().col, 79);
}
