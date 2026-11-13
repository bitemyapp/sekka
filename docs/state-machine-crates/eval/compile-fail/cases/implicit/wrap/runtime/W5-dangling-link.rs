// W5 dangling-link. `wrapped` and `continuation` are separate masks, but
// only a soft wrap sets them, marking the continuation on the next row it
// moves to: refused at run time.
use impl_implicit::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 3, rows: 3 });
    let mut out = Vec::new();
    for _ in 0..4 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    let wrap = out.iter().position(|e| *e == Effect::SoftWrap { row: 0 }).expect("a soft wrap");
    assert_eq!(out[wrap + 1], Effect::Put { row: 1, col: 0, wide: false });
    assert!(m.wrapped & 1 != 0 && m.continuation & 2 != 0);
}
