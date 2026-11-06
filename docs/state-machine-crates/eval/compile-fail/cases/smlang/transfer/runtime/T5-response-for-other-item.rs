// T5 response-for-other-item. Every response row is guarded by its item
// check (`probed`, `answered`, ...); for another item no guard passes and
// `process_event` returns `Err(TransitionsFailed)` (refused at run time).
use impl_smlang::transfer::{Error, Events, States, Transfer};
use spec::transfer::{Config, Found, ItemId, Machine};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    sm.process_event(&mut out, Events::Start(2)).unwrap();
    out.clear();
    assert_eq!(sm.process_event(&mut out, Events::Probed((ItemId(1), Found::Free))), Err(Error::TransitionsFailed));
    assert!(out.is_empty());
    assert!(matches!(sm.state_enum(), States::Probing(ItemId(0))));
}
