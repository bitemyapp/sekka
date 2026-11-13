// W3 clear-on-tab: HT as a control that clears the pending wrap. The controls
// are split: HT is `Keeping::Tab`, and `Screen<Pending>::keeping` returns a
// `Screen<Pending>`; `Clearing` has no tab.
use impl_statum::wrap::{Clearing, Normal, Screen};
use spec::wrap::{Config, Effect};
use statum::Branch;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Branch::Second(pending) = Screen::<Normal>::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    let _ = pending.clearing(Clearing::Tab, &mut out);
}
