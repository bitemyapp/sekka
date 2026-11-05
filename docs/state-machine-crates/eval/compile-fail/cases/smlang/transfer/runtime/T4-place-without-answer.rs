// T4 place-without-answer. A conflict's row asks (`Asking`), and `Asking`
// declares only `Answered`: a placement result there is
// `Err(InvalidEvent)`, and no `PlaceAt` came before the answer (refused at
// run time).
use impl_smlang::transfer::{Error, Events, States, Transfer};
use spec::transfer::{Config, Effect, Found, ItemId, Machine, Placed};

fn main() {
    let mut m = Transfer::new(Config::default());
    let mut out = Vec::new();
    let item = ItemId(0);
    let sm = &mut m;
    sm.process_event(&mut out, Events::Start(1)).unwrap();
    sm.process_event(&mut out, Events::Probed((item, Found::Conflict { can_merge: true }))).unwrap();
    assert!(matches!(sm.state_enum(), States::Asking(_)));
    assert_eq!(sm.process_event(&mut out, Events::Placed((item, Placed::Ok))), Err(Error::InvalidEvent));
    assert!(!out.iter().any(|e| matches!(e, Effect::PlaceAt { .. })));
}
