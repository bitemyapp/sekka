// P2 owner-after-native-drag: the generated `State::NativeDrag` has no state-local owner.
use impl_statig::pointer::{Pointer, State};
use spec::pointer::{Config, Machine};

fn main() {
    let m = Pointer::new(Config::default());
    if let State::NativeDrag { owner } = m.state_enum() {
        let _ = owner;
    }
}
