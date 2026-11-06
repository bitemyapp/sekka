// W1 pending-off-the-edge. By transitions, `PendingWrap` is entered only by
// a print whose `ends_pending` guard says it wrote the last column, by a
// restore of such a cursor (`saved_pending`), or kept across SetAutowrap,
// HT, RI at the top and DECSC: on a 4-column screen three prints stay
// `Normal`, and only the one writing column 3 leaves a wrap pending
// (refused at run time: the guard decides, not the types).
use impl_state_machines::wrap::{Ctx, Screen, ScreenNormalPrintOutcome};
use spec::wrap::{Config, Effect};

fn main() {
    let screen = Screen::new(Ctx::new(Config { cols: 4, rows: 2 }));
    let ScreenNormalPrintOutcome::Normal(screen) = screen.print(Some(false)).unwrap() else { panic!("pending at 1") };
    let ScreenNormalPrintOutcome::Normal(screen) = screen.print(Some(false)).unwrap() else { panic!("pending at 2") };
    let ScreenNormalPrintOutcome::Normal(screen) = screen.print(Some(false)).unwrap() else { panic!("pending at 3") };
    let ScreenNormalPrintOutcome::PendingWrap(mut pending) = screen.print(Some(false)).unwrap() else { panic!() };
    assert_eq!(pending.take_effects().last(), Some(&Effect::Put { row: 0, col: 3, wide: false }));
}
