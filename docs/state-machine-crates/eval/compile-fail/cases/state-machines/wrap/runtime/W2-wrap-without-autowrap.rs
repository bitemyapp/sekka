// W2 wrap-without-autowrap. The typed print from `NoAutowrap`'s leaves runs
// the same callback with `Mode::AUTOWRAP == false`: it overwrites the last
// column and drops a wide scalar that doesn't fit, never wrapping (refused
// at run time: the callback decides, not the types).
use impl_state_machines::wrap::{
    Ctx, Screen, ScreenNormalNoAutowrapPrintOutcome, ScreenNormalSetAutowrapOutcome, ScreenPendingNoAutowrapPrintOutcome,
};
use spec::wrap::{Config, Effect};

fn main() {
    let screen = Screen::new(Ctx::new(Config { cols: 2, rows: 2 }));
    let ScreenNormalSetAutowrapOutcome::NormalNoAutowrap(off) = screen.set_autowrap(false).unwrap() else { panic!() };
    let ScreenNormalNoAutowrapPrintOutcome::NormalNoAutowrap(off) = off.print(Some(false)).unwrap() else { panic!() };
    let ScreenNormalNoAutowrapPrintOutcome::PendingNoAutowrap(pending) = off.print(Some(false)).unwrap() else {
        panic!()
    };
    let ScreenPendingNoAutowrapPrintOutcome::NormalNoAutowrap(mut off) = pending.print(Some(true)).unwrap() else {
        panic!()
    };
    let effects = off.take_effects();
    assert_eq!(
        effects,
        vec![Effect::Put { row: 0, col: 0, wide: false }, Effect::Put { row: 0, col: 1, wide: false }]
    );
    assert!(!effects.iter().any(|e| matches!(e, Effect::SoftWrap { .. } | Effect::SpacerHead { .. })));
}
