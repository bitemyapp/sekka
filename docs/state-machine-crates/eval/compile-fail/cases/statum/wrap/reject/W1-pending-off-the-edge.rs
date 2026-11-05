// W1 pending-off-the-edge: a pending wrap with the cursor at column 0. Only a
// print reaching the last column (or a restore of such a cursor) yields a
// `Screen<Pending>`; a fresh `Screen<Normal>` at column 0 isn't one.
use impl_statum::wrap::screen::SomeState;
use impl_statum::wrap::{Normal, Screen};
use spec::wrap::Config;

fn main() {
    let normal = Screen::<Normal>::new(Config::default());
    let _ = SomeState::Pending(normal);
}
