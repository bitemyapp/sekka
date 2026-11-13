// W4 print-while-pending. `PendingWrap`'s print rows call the print that wraps
// first; `PendingNoAutowrap`'s overwrite the last column. Refused at run
// time.
use impl_sml::wrap::{Print, Wrap, WrapStates};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    for _ in 0..4 {
        sm.process_event(&mut out, Print { wide: false }).unwrap();
    }
    out.clear();
    sm.process_event(&mut out, Print { wide: false }).unwrap();
    assert_eq!(out, [Effect::SoftWrap { row: 0 }, Effect::Put { row: 1, col: 0, wide: false }]);
    assert!(sm.is(&WrapStates::Normal));
}
