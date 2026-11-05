// Forge, W1 pending-off-the-edge. `print` declares the edge `Normal → Pending`,
// so statum implements the public `CanTransitionWith<PendingTok>` for
// `Screen<Normal>`, and taking it skips the method body. Outside code can't
// make a `PendingTok`, but `state_data` is always a public field: it moves
// one out of another screen that legitimately reached `Pending`. A fresh
// screen at column 0 then has a wrap pending, and its next print soft-wraps
// a row that never reached its last column. (The other public means:
// `transition_map` and `CanTransitionMap`, with a closure returning such a
// token.)
use impl_statum::wrap::{Normal, Pending, Screen};
use spec::wrap::{Config, Effect};
use statum::{Branch, CanTransitionWith};

fn main() {
    let mut donor_out: Vec<Effect> = Vec::new();
    let Branch::Second(donor) = Screen::<Normal>::new(Config { cols: 2, rows: 2 }).print(true, &mut donor_out) else {
        panic!("no pending wrap");
    };
    let token = donor.state_data;
    let mut out: Vec<Effect> = Vec::new();
    let pending: Screen<Pending> = Screen::<Normal>::new(Config::default()).transition_with_data(token);
    let _ = pending.print(false, &mut out);
    assert_eq!(out, [Effect::SoftWrap { row: 0 }, Effect::Put { row: 1, col: 0, wide: false }]);
}
