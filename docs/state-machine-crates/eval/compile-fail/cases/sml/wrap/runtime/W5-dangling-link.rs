// W5 dangling-link. The links are private context, set only by the soft
// wrap, which moves to the next row (scrolling at the bottom) before
// anything else is written. Refused at run time.
use impl_sml::wrap::{Print, Wrap};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 1 });
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    for _ in 0..3 {
        sm.process_event(&mut out, Print { wide: false }).unwrap();
    }
    out.clear();
    sm.process_event(&mut out, Print { wide: true }).unwrap();
    assert_eq!(
        out,
        [
            Effect::SpacerHead { row: 0, col: 3 },
            Effect::SoftWrap { row: 0 },
            Effect::ScrollUp,
            Effect::Put { row: 0, col: 0, wide: true },
        ]
    );
}
