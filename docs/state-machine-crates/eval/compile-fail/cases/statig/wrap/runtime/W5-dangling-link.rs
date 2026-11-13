// W5 dangling-link. The links are private shared storage, set only by
// `soft_wrap`, which moves to the next row in the same call: EL on a row no
// soft wrap linked breaks nothing, and the linked row is the one above the
// cursor (refused at run time).
use impl_statig::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 3 });
    let mut out = Vec::new();
    m.handle(Event::EraseLine(0), &mut out);
    assert!(out.is_empty());
    m.handle(Event::Repeat(1), &mut out);
    for _ in 0..5 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    assert_eq!(out[4], Effect::SoftWrap { row: 0 });
    assert_eq!(out[5], Effect::Put { row: 1, col: 0, wide: false });
    out.clear();
    m.handle(Event::EraseLine(2), &mut out);
    assert!(out.is_empty());
    m.handle(Event::ReverseIndex, &mut out);
    m.handle(Event::EraseLine(0), &mut out);
    assert_eq!(out, vec![Effect::Unlink { row: 0 }]);
}
