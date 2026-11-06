// K1 answer-without-prompt. Only `Prompted + Submit` is declared; anywhere
// else the generated `process_event` returns `Err(InvalidEvent)` (refused at
// run time: `Starting` and `Prompted` carry the same `Run` data, so the
// types don't tell them apart).
use impl_smlang::polkit::{Error, Events, Polkit, States};
use spec::polkit::{Config, Cookie, Effect, Machine};

fn main() {
    let mut m = Polkit::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    assert_eq!(sm.process_event(&mut out, Events::Submit), Err(Error::InvalidEvent));
    sm.process_event(&mut out, Events::Begin((Cookie(7), 1, 0, true))).unwrap();
    assert!(matches!(sm.state_enum(), States::Starting(_)));
    assert_eq!(sm.process_event(&mut out, Events::Submit), Err(Error::InvalidEvent));
    assert!(!out.iter().any(|e| matches!(e, Effect::WriteAnswer { .. })));
}
