// I6 closure-on-deferred-route: `Session::Prepared` holds the `Route` alone;
// a closure's size is only `Prep::Cache`'s, on the in-memory route.
use impl_fsmentry::install::{Install, Session};
use spec::install::{Config, Machine};

fn main() {
    let m = Install::new(Config::default());
    if let Session::Prepared(route, closure) = m.session() {
        let _ = (route, closure);
    }
}
