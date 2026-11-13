// I6 closure-on-deferred-route: `State::Prepared` keeps only the route; the
// closure's size lives in `State::Cache`, on the in-memory route.
use impl_enum::install::{Install, State};
use spec::install::{Config, Machine};

fn main() {
    let m = Install::new(Config::default());
    if let State::Prepared { deferred: true, closure } = m.typed() {
        let _ = closure;
    }
}
