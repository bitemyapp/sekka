// W4 print-while-pending: write a cell from `Pending` without wrapping first,
// with the print of a state that has no wrap pending.
use impl_typestate::wrap::{Normal, Ready};
use spec::wrap::{Config, Effect};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Ready::Pending(pending) = Normal::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    let _ = Normal::print(pending, false, &mut out);
}
