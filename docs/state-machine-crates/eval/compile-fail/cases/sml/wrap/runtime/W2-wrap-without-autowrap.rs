// W2 wrap-without-autowrap. Only `Normal` and `PendingWrap`'s print rows call
// the print with autowrap on; the `NoAutowrap` states' rows overwrite the
// last column and drop a wide scalar there. Refused at run time.
use impl_sml::wrap::{Print, SetAutowrap, Wrap, WrapStates};
use spec::wrap::{Config, Effect, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    sm.process_event(&mut out, SetAutowrap(false)).unwrap();
    for wide in [false, false, false, false, false, true] {
        sm.process_event(&mut out, Print { wide }).unwrap();
    }
    assert!(sm.is(&WrapStates::NormalNoAutowrap));
    assert!(!out.iter().any(|e| matches!(e, Effect::SoftWrap { .. } | Effect::SpacerHead { .. })));
}
