// W5 dangling-link: mark the row soft-wrapped and stay on it. The links are a
// private field of the `#[machine]`; the only soft wraps are inside
// `Screen<Pending>::print` (and a wide print at the edge), which move to the
// next row and write there.
use impl_statum::wrap::{Normal, Screen};
use spec::wrap::{Config, Effect};
use statum::Branch;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Branch::Second(pending) = Screen::<Normal>::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    pending.soft_wrap(&mut out);
}
