// W3 clear-on-tab: HT as a control that clears the pending wrap.
// impl-typestate splits the controls: HT is `Keeping::Tab`, and
// `Pending::keeping` returns a `Pending`; `Clearing` has no tab.
use impl_typestate::wrap::{Clearing, Normal, Ready};
use spec::wrap::{Config, Effect};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Ready::Pending(pending) = Normal::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    let _ = pending.clearing(Clearing::Tab, &mut out);
}
