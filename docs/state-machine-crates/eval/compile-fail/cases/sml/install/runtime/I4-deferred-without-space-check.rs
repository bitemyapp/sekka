// I4 deferred-without-space-check. `Prepared` is reached only through the
// parent's `event<Ready>` row, and only the table's own `process(Ready(..))`
// sends a `Ready`: its field is private, so outside code can't build one
// (`Ready(true)` here is E0423). On the deferred route `Evaluation`'s guards
// check the root's room first: a selection that doesn't fit ends `Refused`,
// never `Prepared`, and the confirmation then finds no row. Refused at run
// time.
use impl_sml::install::{Evaluated, Install, InstallEvents as E};
use spec::install::{Config, Effect, GIB, Machine, StateName};

fn main() {
    let mut m = Install::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    for event in [E::Begin, E::Checked(39 * GIB), E::Configured] {
        m.machine.process_event(&mut out, event).unwrap();
    }
    out.clear();
    let eval = Evaluated { download_mib: 60 * GIB, unpacked_mib: 300 * GIB, budget_mib: 20 * GIB };
    m.machine.process_event(&mut out, E::Evaluated(eval)).unwrap();
    assert_eq!(out, [Effect::Refused { needed_mib: 308 * GIB, root_mib: 39 * GIB }]);
    assert_eq!(m.state(), StateName::Refused);
    assert!(m.machine.process_event(&mut out, E::Confirm(true)).is_err());
    assert_eq!(out.len(), 1);
}
