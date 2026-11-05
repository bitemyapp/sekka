// W2 wrap-without-autowrap: soft-wrap a pending wrap with autowrap off. Only
// `Screen<Pending>` (autowrap on) has the print that wraps first;
// `Screen<PendingNoWrap>::print` overwrites the last column.
use impl_statum::wrap::{Normal, Pending, Screen};
use spec::wrap::{Config, Effect};
use statum::Branch;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Branch::Second(pending) = Screen::<Normal>::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    let off = pending.autowrap_off();
    let _ = Screen::<Pending>::print(off, false, &mut out);
}
