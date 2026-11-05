// W1 pending-off-the-edge. The `Mode` graph knows nothing of the cursor:
// `Normal::pend` takes no column. Through the live machine's handles a
// pending wrap is set at column 0, and the next print soft-wraps a row that
// never reached its last column.
use impl_fsmentry::wrap::{ModeEntry, Wrap};
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config::default());
    if let ModeEntry::Normal(normal) = m.entry() {
        normal.pend();
    }
    assert_eq!(m.state(), StateName::PendingWrap);
    let mut out = Vec::new();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, vec![Effect::SoftWrap { row: 0 }, Effect::Put { row: 1, col: 0, wide: false }]);
}
