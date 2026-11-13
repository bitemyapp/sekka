// W1 pending-off-the-edge. The rows into `PendingWrap` are guarded by
// `fills_*`, which pass only when the print reaches the last column (narrow
// or wide, after a wrap or not); `RestoreCursor` restores a pending wrap
// only with the column it was saved at (refused at run time).
use impl_smlang::wrap::{Events, States, Wrap};
use spec::wrap::{Config, Machine};

fn main() {
    let mut m = Wrap::new(Config { cols: 4, rows: 3 });
    let mut out = Vec::new();
    let prints = [false, true, false, true, true, false, false, true];
    for (i, wide) in prints.into_iter().enumerate() {
        if i == 3 {
            m.process_event(&mut out, Events::SaveCursor).unwrap();
        }
        m.process_event(&mut out, Events::Print(wide)).unwrap();
        if matches!(m.state_enum(), States::PendingWrap) {
            assert_eq!(m.context().position().1, 3);
        }
    }
    m.process_event(&mut out, Events::CursorTo((0, 0))).unwrap();
    m.process_event(&mut out, Events::RestoreCursor).unwrap();
    if matches!(m.state_enum(), States::PendingWrap) {
        assert_eq!(m.context().position().1, 3);
    }
}
