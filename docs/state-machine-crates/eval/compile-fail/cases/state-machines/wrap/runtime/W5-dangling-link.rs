// W5 dangling-link. The links are private context, set only by the
// soft-wrap step, which always moves to the next row (scrolling at the
// bottom): each `SoftWrap` is followed by writing on the continuation row
// (refused at run time: the callback, not the types, keeps them together).
use impl_state_machines::wrap::{Ctx, Screen, ScreenNormalPrintOutcome, ScreenPendingWrapPrintOutcome};
use spec::wrap::{Config, Effect};

fn main() {
    let screen = Screen::new(Ctx::new(Config { cols: 2, rows: 2 }));
    let ScreenNormalPrintOutcome::PendingWrap(pending) = screen.print(Some(true)).unwrap() else { panic!() };
    let ScreenPendingWrapPrintOutcome::PendingWrap(pending) = pending.print(Some(true)).unwrap() else { panic!() };
    let ScreenPendingWrapPrintOutcome::PendingWrap(mut pending) = pending.print(Some(true)).unwrap() else { panic!() };
    assert_eq!(
        pending.take_effects(),
        vec![
            Effect::Put { row: 0, col: 0, wide: true },
            Effect::SoftWrap { row: 0 },
            Effect::Put { row: 1, col: 0, wide: true },
            Effect::SoftWrap { row: 1 },
            Effect::ScrollUp,
            Effect::Put { row: 1, col: 0, wide: true },
        ]
    );
}
