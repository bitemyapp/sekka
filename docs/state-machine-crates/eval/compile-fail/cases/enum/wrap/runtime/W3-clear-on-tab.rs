// W3 clear-on-tab. impl-enum's `handle` leaves the state alone on HT: the
// pending wrap is kept (refused at run time).
use impl_enum::wrap::Wrap;
use spec::wrap::{Config, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    m.handle(Event::CursorTo { row: 0, col: 3 }, &mut out);
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(m.state(), StateName::PendingWrap);
    m.handle(Event::Tab, &mut out);
    assert_eq!(m.state(), StateName::PendingWrap);
}
