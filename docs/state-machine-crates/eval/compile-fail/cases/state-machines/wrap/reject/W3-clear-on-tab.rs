// W3 clear-on-tab: `tab` is an internal transition, so the typed
// `Screen<PendingWrap>::tab` returns a `Screen<PendingWrap>`, not a
// `Screen<Normal>`.
use impl_state_machines::wrap::{Ctx, Normal, Screen, ScreenNormalPrintOutcome};
use spec::wrap::Config;

fn main() {
    let screen = Screen::new(Ctx::new(Config { cols: 2, rows: 2 }));
    let ScreenNormalPrintOutcome::PendingWrap(pending) = screen.print(Some(true)).unwrap() else { return };
    let _cleared: Screen<Normal> = pending.tab().unwrap();
}
