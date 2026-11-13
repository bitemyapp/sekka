// W3 clear-on-tab. The `Tab` row is internal in every state (`_ + event<Tab>
// / tab`, no target): it can't leave a pending state. Refused at run time.
use impl_sml::wrap::{Print, Tab, Wrap, WrapStates};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    for _ in 0..4 {
        sm.process_event(&mut out, Print { wide: false }).unwrap();
    }
    sm.process_event(&mut out, Tab).unwrap();
    assert!(sm.is(&WrapStates::PendingWrap));
    out.clear();
    sm.process_event(&mut out, Print { wide: false }).unwrap();
    assert_eq!(out[0], Effect::SoftWrap { row: 0 });
}
