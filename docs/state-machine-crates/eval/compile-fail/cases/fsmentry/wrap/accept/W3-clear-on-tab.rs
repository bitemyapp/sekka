// W3 clear-on-tab. fsmentry allows one edge per pair of states, so
// `PendingWrap -settle-> Normal` serves CR, LF, BS, CUP, RI, EL, DECRC and
// the wrapping print alike: it can't stand for any one of them, and no edge
// can take the event as an argument (only the destination's data, and
// `Normal` has none). An HT handler can take it: after `Tab` the pending
// wrap is cleared through the handle, and the next print overwrites the
// last column instead of wrapping.
use impl_fsmentry::wrap::{ModeEntry, Wrap};
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    m.handle(Event::CursorTo { row: 0, col: 3 }, &mut out);
    m.handle(Event::Print { wide: false }, &mut out);
    m.handle(Event::Tab, &mut out);
    if let ModeEntry::PendingWrap(pending) = m.entry() {
        pending.settle();
    }
    assert_eq!(m.state(), StateName::Normal);
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, vec![Effect::Put { row: 0, col: 3, wide: false }]);
}
