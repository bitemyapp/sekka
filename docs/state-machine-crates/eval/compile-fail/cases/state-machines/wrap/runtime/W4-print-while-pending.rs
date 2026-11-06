// W4 print-while-pending. A print from the typed `Screen<PendingWrap>`
// wraps first (`Mode::PENDING`), then writes on the next row (refused at
// run time: the order is the callback's).
use impl_state_machines::wrap::{Ctx, Screen, ScreenNormalPrintOutcome, ScreenPendingWrapPrintOutcome};
use spec::wrap::{Config, Effect};

fn main() {
    let screen = Screen::new(Ctx::new(Config { cols: 2, rows: 2 }));
    let ScreenNormalPrintOutcome::PendingWrap(mut pending) = screen.print(Some(true)).unwrap() else { panic!() };
    assert_eq!(pending.take_effects(), vec![Effect::Put { row: 0, col: 0, wide: true }]);
    let ScreenPendingWrapPrintOutcome::Normal(mut normal) = pending.print(Some(false)).unwrap() else { panic!() };
    assert_eq!(normal.take_effects(), vec![Effect::SoftWrap { row: 0 }, Effect::Put { row: 1, col: 0, wide: false }]);
}
