// W3 clear-on-tab. `Tab` is one wildcard internal row (`_ + Tab / tab`):
// every state stays where it is, so a pending wrap survives it (refused at
// run time).
use impl_smlang::wrap::{Events, States, Wrap};
use spec::wrap::{Config, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::CursorTo((0, 3))).unwrap();
    sm.process_event(&mut out, Events::Print(false)).unwrap();
    assert!(matches!(sm.state_enum(), States::PendingWrap));
    sm.process_event(&mut out, Events::Tab).unwrap();
    assert!(matches!(sm.state_enum(), States::PendingWrap));
}
