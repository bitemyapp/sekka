// P1 release-without-owner. finny picks the transition by (state, event):
// `Inside`'s `Hovering` handles a release by tracking the hover, and from
// `Outside` the release enters `Inside` and is handled there; the typed
// `dispatch` of `Inside`'s release while `Outside` is `Err(NoTransition)`.
// No `Up` (refused at run time; nothing in the types forbids an action from
// pushing one).
use impl_finny::finny::FsmError;
use impl_finny::pointer::{ButtonUp, InsideEvents, Pointer};
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Point, StateName};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out = Vec::new();
    let up = ButtonUp { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) };
    assert_eq!(m.dispatch(InsideEvents::from(up), &mut out), Err(FsmError::NoTransition));
    m.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    assert_eq!(m.state(), StateName::Hovering);
    m.handle(Event::Move { at: Point::new(0, 0), hit: Hit::NOTHING }, &mut out);
    m.handle(Event::Release { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) }, &mut out);
    assert!(!out.iter().any(|e| matches!(e, Effect::Up { .. })));
}
