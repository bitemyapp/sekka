// W1 pending-off-the-edge. The pending wrap is the state (`State::PendingWrap
// {}`), the cursor is private shared storage, and only a print that reaches
// the last column enters it; every control that moves the cursor off that
// column leaves it (refused at run time). A variant literal can be built, but
// not installed: the machine is private, and statig's
// `InitializedStateMachine::state_mut` is `unsafe`.
use impl_statig::wrap::{State, Wrap};
use spec::wrap::{Config, Effect, Event, Machine, StateName};

fn main() {
    let _unattached = State::PendingWrap {};
    let mut m = Wrap::new(Config { cols: 4, rows: 2 });
    let mut out = Vec::new();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(m.state(), StateName::Normal);
    m.handle(Event::CursorTo { row: 0, col: 3 }, &mut out);
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(*m.state_enum(), State::PendingWrap {});
    m.handle(Event::Backspace, &mut out);
    assert_eq!(m.state(), StateName::Normal);
    out.clear();
    m.handle(Event::Print { wide: false }, &mut out);
    assert_eq!(out, vec![Effect::Put { row: 0, col: 2, wide: false }]);
}
