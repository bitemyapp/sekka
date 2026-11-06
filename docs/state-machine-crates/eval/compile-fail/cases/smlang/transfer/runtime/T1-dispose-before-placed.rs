// T1 dispose-before-placed. Only `PlacingReplacement + Placed` with
// `Placed::Ok` disposes of the aside; an early `Disposed` is
// `Err(InvalidEvent)` and a failed placement restores it (refused at run
// time).
use impl_smlang::transfer::{Error, Events, States, Transfer};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, Machine, Placed, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    let sm = &mut m;
    sm.process_event(&mut out, Events::Start(1)).unwrap();
    sm.process_event(&mut out, Events::Probed((item, Found::Conflict { can_merge: false }))).unwrap();
    sm.process_event(&mut out, Events::Answered((item, Resolution::Replace))).unwrap();
    sm.process_event(&mut out, Events::SetAside((item, Some(AsideId(5))))).unwrap();
    assert_eq!(sm.process_event(&mut out, Events::Disposed((item, true))), Err(Error::InvalidEvent));
    assert!(matches!(sm.state_enum(), States::PlacingReplacement(_)));
    sm.process_event(&mut out, Events::Placed((item, Placed::Skip))).unwrap();
    assert!(!out.iter().any(|e| matches!(e, Effect::Trash { .. } | Effect::Delete { .. })));
    assert!(matches!(sm.state_enum(), States::Restoring(_)));
}
