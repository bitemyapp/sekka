// W4 print-while-pending. Both `PendingWrap + Print` rows run
// `print_pending`, which soft-wraps before writing: the cell goes to the
// next row (refused at run time).
use impl_smlang::wrap::{Events, States, Wrap};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::CursorTo((0, 3))).unwrap();
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    assert!(matches!(sm.state_enum(), States::PendingWrap));
    out.clear();
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    assert_eq!(out, vec![Effect::SoftWrap { row: 0 }, Effect::Put { row: 1, col: 0, wide: false }]);
}
