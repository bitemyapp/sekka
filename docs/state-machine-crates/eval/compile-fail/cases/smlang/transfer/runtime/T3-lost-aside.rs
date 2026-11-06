// T3 lost-aside. `Restoring` declares only `Restored` (guarded by its item):
// every other response, and a `Restored` for another item, is an `Err`, so
// the aside can't be dropped (refused at run time).
use impl_smlang::transfer::{Error, Events, States, Transfer};
use spec::transfer::{AsideId, Config, Effect, Found, ItemId, Machine, Placed, Resolution};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    m.process_event(&mut out, Events::Start(2)).unwrap();
    m.process_event(&mut out, Events::Probed((item, Found::Conflict { can_merge: false }))).unwrap();
    m.process_event(&mut out, Events::Answered((item, Resolution::Replace))).unwrap();
    m.process_event(&mut out, Events::SetAside((item, Some(AsideId(5))))).unwrap();
    m.process_event(&mut out, Events::Placed((item, Placed::Skip))).unwrap();
    assert!(matches!(m.state_enum(), States::Restoring(_)));
    out.clear();
    let next = ItemId(1);
    assert_eq!(m.process_event(&mut out, Events::Probed((next, Found::Same))), Err(Error::InvalidEvent));
    assert_eq!(m.process_event(&mut out, Events::Disposed((item, true))), Err(Error::InvalidEvent));
    assert_eq!(m.process_event(&mut out, Events::Restored((next, true))), Err(Error::TransitionsFailed));
    assert!(out.is_empty());
    m.process_event(&mut out, Events::Restored((item, false))).unwrap();
    assert_eq!(out[0], Effect::Problem { item, problem: spec::transfer::Problem::Stranded(AsideId(5)) });
}
