// W2 wrap-without-autowrap. The print helper wraps only when the mode
// region's mirror says autowrap, so in `NoAutowrap` a print over the last
// column overwrites it and a wide one is dropped: no `SoftWrap` or
// `SpacerHead` (refused at run time).
use impl_finny::wrap::Wrap;
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
