// W5 dangling-link. A row is linked only by the soft wrap, which line-feeds
// before the next cell: every `SoftWrap { row }` is followed by a `Put` on
// `row + 1`, and erasing a row that never wrapped unlinks nothing (refused
// at run time).
use impl_finny::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 3 });
    let mut out = Vec::new();
    for _ in 0..5 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    m.handle(Event::Print { wide: true }, &mut out);
    m.handle(Event::Print { wide: true }, &mut out);
    let wraps: Vec<usize> = (0..out.len()).filter(|&i| matches!(out[i], Effect::SoftWrap { .. })).collect();
    assert_eq!(wraps.len(), 2);
    for i in wraps {
        let Effect::SoftWrap { row } = out[i] else { unreachable!() };
        let next_put = out[i..].iter().find_map(|e| if let Effect::Put { row, .. } = e { Some(*row) } else { None });
        assert_eq!(next_put, Some(row + 1));
    }
    out.clear();
    m.handle(Event::CursorTo { row: 2, col: 0 }, &mut out);
    m.handle(Event::EraseLine(0), &mut out);
    assert!(out.is_empty());
    m.handle(Event::CursorTo { row: 0, col: 0 }, &mut out);
    m.handle(Event::EraseLine(0), &mut out);
    assert_eq!(out, vec![Effect::Unlink { row: 0 }]);
}
