// Accepted: print through the typed API, committed back.
use impl_typestate::wrap::{Any, Wrap};
use spec::wrap::{Config, Effect, Machine, StateName};
fn main() {
    let mut w = Wrap::new(Config { cols: 2, rows: 2 });
    let mut out: Vec<Effect> = Vec::new();
    w.with_typed(|s| match s {
        Any::Normal(n) => (n.print(true, &mut out).into(), ()),
        other => (other, ()),
    });
    assert_eq!(w.state(), StateName::PendingWrap);
}
