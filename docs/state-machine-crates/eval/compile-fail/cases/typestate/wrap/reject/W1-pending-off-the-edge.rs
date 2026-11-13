// W1 pending-off-the-edge: a pending wrap with the cursor at column 0.
// In impl-typestate only a print reaching the last column (or a restore of
// such a cursor) yields a `Pending`; a fresh `Normal` at column 0 isn't one.
use impl_typestate::wrap::{Any, Normal};
use spec::wrap::Config;

fn main() {
    let normal = Normal::new(Config::default());
    let _ = Any::Pending(normal);
}
