// I4 deferred-without-space-check. On the deferred route the `Evaluation`
// rows into `Prepared` are guarded by `room_unpacked`; without room the last
// row refuses (refused at run time).
use impl_smlang::install::{Events, Install, States};
use spec::install::{Config, Effect, GIB, Machine};

fn main() {
    let mut m = Install::new(Config::default());
    m.process_event(Events::Begin).unwrap();
    m.process_event(Events::Checked(39 * GIB)).unwrap();
    m.process_event(Events::Configured).unwrap();
    m.process_event(Events::Evaluated((60 * GIB, 300 * GIB, 16 * GIB))).unwrap();
    assert!(matches!(m.state_enum(), States::Refused));
    let pending = m.context().pending();
    assert!(pending.contains(&Effect::Refused { needed_mib: 308 * GIB, root_mib: 39 * GIB }));
    assert!(!pending.iter().any(|e| matches!(e, Effect::Prepared { .. })));
}
