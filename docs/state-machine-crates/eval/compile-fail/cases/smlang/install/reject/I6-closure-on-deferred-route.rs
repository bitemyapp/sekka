// I6 closure-on-deferred-route: `States::Prepared` carries only the
// `Route`; the closure's size is `States::Cache`'s data, on the in-memory
// route.
use impl_smlang::install::{Install, Route, States};
use spec::install::{Config, Machine};

fn main() {
    let mut m = Install::new(Config::default());
    if let States::Prepared(Route { deferred: true, closure }) = m.state_enum() {
        let _ = closure;
    }
}
