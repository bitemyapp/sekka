// P4 press-point-outside-edge-press: `Press::Owned` keeps no press point.
use impl_enum::pointer::{Pointer, Press, State};
use spec::pointer::{Config, Machine};

fn main() {
    let m = Pointer::new(Config::default());
    if let State::Inside { press: Press::Owned { owner, at }, .. } = m.typed() {
        let _ = (owner, at);
    }
}
