// P2 owner-after-native-drag: `State::NativeDrag` carries no owner.
use impl_enum::pointer::{Pointer, State};
use spec::pointer::{Config, Machine};

fn main() {
    let m = Pointer::new(Config::default());
    if let State::NativeDrag { owner } = m.typed() {
        let _ = owner;
    }
}
