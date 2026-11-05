use impl_typestate::wrap::{Any, Wrap};
use spec::wrap::{Config, Machine};
fn main() {
    let mut w = Wrap::new(Config::default());
    w.with_typed(|s| match s {
        Any::Normal(n) => (Any::Pending(n), ()),
        other => (other, ()),
    });
}
