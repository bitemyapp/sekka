// Hand machine A's typed state back to machine B.
use impl_typestate::wrap::Wrap;
use spec::wrap::{Config, Machine};
fn main() {
    let mut a = Wrap::new(Config::default());
    let mut b = Wrap::new(Config::default());
    a.with_typed::<()>(|sa| {
        b.with_typed::<()>(|_sb| (sa, ()));
        unreachable!()
    });
}
