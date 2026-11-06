use impl_typestate::wrap::{Any, Wrap};
use spec::wrap::{Config, Effect, Machine};
fn main() {
    let mut w = Wrap::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    w.with_typed(|s| match s {
        Any::Pending(p) => { p.soft_wrap(&mut out); (Any::Pending(p), ()) }
        other => (other, ()),
    });
}
