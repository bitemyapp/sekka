use impl_typestate::wrap::{Any, Normal, Wrap};
use spec::wrap::{Config, Effect, Machine};
fn main() {
    let mut w = Wrap::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    w.with_typed(|s| match s {
        Any::Pending(p) => (Normal::print(p, false, &mut out).into(), ()),
        other => (other, ()),
    });
}
