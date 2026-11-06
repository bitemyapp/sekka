// I4 deferred-without-space-check. The sizes are data beside the table, so
// the room check is a guard in `handle`'s input function: the table only
// sees `EvaluatedDeferred` (room) or `EvaluatedNoRoom` (a bare table would
// take `EvaluatedDeferred` unchecked). Through the machine the deferred
// route with no room is refused at run time.
use impl_rust_fsm::install::Install;
use spec::install::{Config, Effect, Event, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Begin, &mut out);
    m.handle(Event::Checked { root_mib: 39 * GIB }, &mut out);
    m.handle(Event::Configured, &mut out);
    m.handle(Event::Evaluated { download_mib: 60 * GIB, unpacked_mib: 300 * GIB, budget_mib: 16 * GIB }, &mut out);
    assert_eq!(m.state(), StateName::Refused);
    assert!(!out.iter().any(|e| matches!(e, Effect::Prepared { .. })));
}
