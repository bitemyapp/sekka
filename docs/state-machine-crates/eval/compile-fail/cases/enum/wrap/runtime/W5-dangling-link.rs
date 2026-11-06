// W5 dangling-link. The link mask is private and set only by a soft wrap,
// which always moves to the next row before writing: refused at run time.
use impl_enum::wrap::Wrap;
use spec::wrap::{Config, Effect, Event, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 3, rows: 3 });
    let mut out = Vec::new();
    // No soft wrap yet: EL finds no link to break.
    m.handle(Event::EraseLine(0), &mut out);
    assert!(out.is_empty());
    for _ in 0..4 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    let wrap = out.iter().position(|e| *e == Effect::SoftWrap { row: 0 }).expect("a soft wrap");
    assert_eq!(out[wrap + 1], Effect::Put { row: 1, col: 0, wide: false });
}
