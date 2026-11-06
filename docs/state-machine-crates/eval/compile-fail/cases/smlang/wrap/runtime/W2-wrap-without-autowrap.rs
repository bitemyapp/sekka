// W2 wrap-without-autowrap. The `NormalNoAutowrap | PendingNoAutowrap +
// Print` rows run `print_no_autowrap`, which overwrites the last column and
// drops a wide scalar there: no soft wrap or spacer (refused at run time).
use impl_smlang::wrap::{Events, States, Wrap};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::SetAutowrap(false)).unwrap();
    sm.process_event(&mut out, Events::CursorTo((0, 3))).unwrap();
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    assert!(matches!(sm.state_enum(), States::PendingNoAutowrap));
    sm.process_event(&mut out, Events::Print(true)).unwrap();
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    assert!(!out.iter().any(|e| matches!(e, Effect::SoftWrap { .. } | Effect::SpacerHead { .. })));
    assert_eq!(sm.context().position(), (0, 3));
}
