// W2 wrap-without-autowrap. Driven through `consume`, the table maps a
// print in the no-autowrap states to `PrintNoWrap`, never `WrapPrint`:
// refused at run time.
use impl_rust_fsm::wrap::Wrap;
use impl_rust_fsm::wrap::fsm::{Input, Output, StateMachine};
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    for path in [&[Input::AutowrapOff][..], &[Input::Reached, Input::AutowrapOff][..]] {
        let mut t = reach(path);
        assert!(matches!(t.state(), StateName::NormalNoAutowrap | StateName::PendingNoAutowrap));
        assert_eq!(t.consume(&Input::Print).ok(), Some(Some(Output::PrintNoWrap)));
    }
    let mut m = Wrap::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::SetAutowrap(false), &mut out);
    for _ in 0..81 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    m.handle(Event::Print { wide: true }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::SoftWrap { .. } | Effect::SpacerHead { .. })));
}
