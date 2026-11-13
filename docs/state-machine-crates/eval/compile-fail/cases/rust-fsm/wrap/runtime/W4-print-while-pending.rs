// W4 print-while-pending. Driven through `consume`, the table resolves a
// pending wrap on every print: `PendingWrap` prints through `WrapPrint`
// (soft wrap first) and `PendingNoAutowrap` drops the flag first. Refused
// at run time.
use impl_rust_fsm::wrap::Wrap;
use impl_rust_fsm::wrap::fsm::{Input, Output, StateMachine};
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let mut t = StateMachine::new();
    t.consume(&Input::Reached).expect("a row");
    assert_eq!(t.consume(&Input::Print).ok(), Some(Some(Output::WrapPrint)));
    assert_eq!(*t.state(), StateName::Normal);
    let mut t = StateMachine::new();
    t.consume(&Input::Reached).expect("a row");
    t.consume(&Input::AutowrapOff).expect("a row");
    assert_eq!(t.consume(&Input::Print).ok(), Some(Some(Output::PrintNoWrap)));
    assert_eq!(*t.state(), StateName::NormalNoAutowrap);

    let mut m = Wrap::new(Config::default());
    let mut out = Vec::new();
    for _ in 0..80 {
        m.handle(Event::Print { wide: false }, &mut out);
    }
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, [Effect::SoftWrap { row: 0 }, Effect::Put { row: 1, col: 0, wide: false }]);
}
