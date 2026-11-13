// W2 wrap-without-autowrap: soft-wrap a pending wrap with autowrap off.
// Only `Pending` (autowrap on) has the print that wraps first;
// `PendingNoWrap::print` overwrites the last column.
use impl_typestate::wrap::{Normal, Pending, Ready};
use spec::wrap::{Config, Effect};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Ready::Pending(pending) = Normal::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    let off = pending.autowrap_off();
    let _ = Pending::print(off, false, &mut out);
}
