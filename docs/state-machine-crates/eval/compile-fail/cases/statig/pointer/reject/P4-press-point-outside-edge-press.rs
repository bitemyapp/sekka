// P4 press-point-outside-edge-press: `State::Pressed` keeps no press point.
use impl_statig::pointer::{Pointer, State};
use spec::pointer::{Config, Machine};

fn main() {
    let m = Pointer::new(Config::default());
    if let State::Pressed { at, .. } = m.state_enum() {
        let _ = at;
    }
}
