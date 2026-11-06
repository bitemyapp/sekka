// P1 release-without-owner. Only the owner states' `Release` rows deliver an
// `Up`: from `Outside` the release enters `Inside` and `Hovering`'s row only
// tracks the hover; in `NativeDrag` no row takes it (`Err`). Refused at run
// time (nothing in the types stops a callback from pushing an `Up`).
use impl_sml::pointer::{Pointer, PointerInsideStates, Press, Release, StartDrag};
use spec::pointer::{Button, Config, Effect, Hit, Machine, Point};

fn main() {
    let mut m = Pointer::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    let sm = &mut m.machine;
    let release = Release { at: Point::new(5, 5), button: Button::Left, hit: Hit::on(2) };
    sm.process_event(&mut out, release).unwrap();
    sm.process_event(&mut out, Press { at: Point::new(0, 0), button: Button::Left, clicks: 1, hit: Hit::NOTHING })
        .unwrap();
    sm.process_event(&mut out, release).unwrap();
    assert!(sm.is(&PointerInsideStates::Hovering));
    sm.process_event(&mut out, StartDrag).unwrap();
    assert!(sm.process_event(&mut out, release).is_err());
    assert!(!out.iter().any(|e| matches!(e, Effect::Up { .. })));
}
