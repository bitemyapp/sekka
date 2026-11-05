// W3 clear-on-tab. `Pending`'s `Tab` is an internal transition: the pending
// region stays in `Pending` (refused at run time; a `transition_to::<Normal>`
// there would compile as well).
use impl_finny::wrap::Wrap;
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
