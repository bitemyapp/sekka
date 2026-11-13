// I6 closure-on-deferred-route: `Prepared`'s payload is the route alone; the
// closure's size is `Cache`'s payload, on the in-memory route.
use impl_sml::install::{Install, InstallStates};
use spec::install::{Config, Machine};

fn main() {
    let m = Install::new(Config::default());
    m.machine.visit_current_states(|state| {
        if let InstallStates::Prepared(true, closure) = state {
            let _ = closure;
        }
    });
}
