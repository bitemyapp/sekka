// W5 dangling-link. The row links are private context data: only the
// print rows that soft-wrap set one, and the same action moves to the next
// row. No event links a row in place, so EL on an unwrapped row unlinks
// nothing (refused at run time).
use impl_smlang::wrap::{Events, Wrap};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::CursorTo((0, 3))).unwrap();
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    sm.process_event(&mut out, Events::EraseLine(0)).unwrap();
    assert_eq!(sm.context().links(), 0);
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    assert_eq!(sm.context().links(), 0);
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    assert_eq!(sm.context().links(), 1);
    assert_eq!(sm.context().position(), (1, 1));
    assert!(!out.iter().any(|e| matches!(e, Effect::Unlink { .. })));
}
