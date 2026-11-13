// W1 pending-off-the-edge. `PendingWrap` is entered only by a print row whose
// guard found that the print ends in the last column, and every row that
// moves the cursor off it leaves the pending states; the cursor is private
// context with no setter. Refused at run time.
use impl_sml::wrap::{Backspace, Print, Tab, Wrap, WrapStates};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    for _ in 0..4 {
        sm.process_event(&mut out, Print { wide: false }).unwrap();
    }
    assert!(sm.is(&WrapStates::PendingWrap));
    sm.process_event(&mut out, Backspace).unwrap();
    assert!(sm.is(&WrapStates::Normal));
    sm.process_event(&mut out, Tab).unwrap();
    assert!(sm.is(&WrapStates::Normal));
    out.clear();
    sm.process_event(&mut out, Print { wide: false }).unwrap();
    assert_eq!(out, [Effect::Put { row: 0, col: 3, wide: false }]);
    assert!(sm.is(&WrapStates::PendingWrap));
}
