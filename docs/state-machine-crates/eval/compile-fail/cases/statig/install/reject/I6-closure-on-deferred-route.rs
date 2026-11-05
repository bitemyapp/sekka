// I6 closure-on-deferred-route: the state-local storage of `Prepared` is the
// route alone; the closure's size lives in `Cache`, on the in-memory route.
use impl_statig::install::{Install, State};
use spec::install::{Config, Machine};

fn main() {
    let m = Install::new(Config::default());
    if let State::Prepared { deferred: true, closure } = m.state_enum() {
        let _ = closure;
    }
}
