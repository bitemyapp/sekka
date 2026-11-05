// W4 print-while-pending: write a cell from `Screen<Pending>` without wrapping
// first, with the print of a state that has no wrap pending.
use impl_statum::wrap::{Normal, Screen};
use spec::wrap::{Config, Effect};
use statum::Branch;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Branch::Second(pending) = Screen::<Normal>::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    let _ = Screen::<Normal>::print(pending, false, &mut out);
}
