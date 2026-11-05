// W2 wrap-without-autowrap. `wrap` and the wide-at-the-edge path both test
// `autowrap` at run time: with it off the pending print overwrites the last
// column and a wide scalar at the edge is dropped (refused at run time).
use impl_implicit::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    m.handle(Event::SetAutowrap(false), &mut out);
    m.handle(Event::CursorTo { row: 0, col: 3 }, &mut out);
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(m.state(), StateName::PendingNoAutowrap);
    m.handle(Event::Print { wide: false }, &mut out);
    m.handle(Event::Print { wide: true }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::SoftWrap { .. } | Effect::SpacerHead { .. })));
    assert_eq!(m.state(), StateName::NormalNoAutowrap);
}
