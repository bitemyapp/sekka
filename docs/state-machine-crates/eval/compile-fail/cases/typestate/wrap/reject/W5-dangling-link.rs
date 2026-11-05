// W5 dangling-link: mark the row soft-wrapped and stay on it.
// The links are private to the screen; the only soft wrap is inside
// `Pending::print` (and a wide print at the edge), which moves to the next
// row and writes there. No state offers a soft wrap of its own.
use impl_typestate::wrap::{Normal, Ready};
use spec::wrap::{Config, Effect};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let Ready::Pending(pending) = Normal::new(Config { cols: 2, rows: 2 }).print(true, &mut out) else {
        return;
    };
    let _ = pending.soft_wrap(&mut out);
}
