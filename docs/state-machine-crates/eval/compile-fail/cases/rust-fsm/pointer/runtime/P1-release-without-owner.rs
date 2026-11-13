// P1 release-without-owner. Driven through `consume` from the initial
// state, the generated table maps a release in `Outside` or `Hovering` to
// the `Track` action, never `Up`: refused at run time by the table's
// transition function (the owner isn't in the table at all).
use impl_rust_fsm::pointer::Pointer;
use impl_rust_fsm::pointer::fsm::{Input, Output, StateMachine};
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Point, StateName};

fn reach(path: &[Input]) -> StateMachine {
    let mut t = StateMachine::new();
    for i in path {
        t.consume(i).expect("a row of the table");
    }
    t
}

fn main() {
    for path in [&[][..], &[Input::Move][..]] {
        let mut t = reach(path);
        assert!(matches!(t.state(), StateName::Outside | StateName::Hovering));
        assert_eq!(t.consume(&Input::Release).ok(), Some(Some(Output::Track)));
        assert_eq!(*t.state(), StateName::Hovering);
    }
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    m.handle(Event::Move { at: Point::new(0, 0), hit: Hit::NOTHING }, &mut out);
    m.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Up { .. })));
}
