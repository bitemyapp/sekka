// W1 pending-off-the-edge. `Pending` is entered only by a print that lands
// in the last column (its guard, `lands_pending`) or by restoring a pending
// save; `Tab` keeps it, every move leaves it. Whenever the state is
// `PendingWrap` the cursor is in the last column (refused at run time).
use impl_finny::wrap::Wrap;
use spec::wrap::{Config, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    let events = [
        Event::Print { wide: false },
        Event::Print { wide: true },
        Event::Print { wide: false },
        Event::SaveCursor,
        Event::CursorTo { row: 0, col: 1 },
        Event::RestoreCursor,
        Event::Tab,
        Event::Backspace,
        Event::Print { wide: true },
        Event::SetAutowrap(false),
        Event::Print { wide: false },
        Event::Repeat(3),
    ];
    let mut pending = 0;
    for event in events {
        m.handle(event, &mut out);
        if matches!(m.state(), StateName::PendingWrap | StateName::PendingNoAutowrap) {
            pending += 1;
            assert_eq!(m.fsm().context.col(), 3, "after {event:?}");
        }
    }
    assert!(pending >= 5);
}
