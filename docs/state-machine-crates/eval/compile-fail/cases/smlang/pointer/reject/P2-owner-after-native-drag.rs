// P2 owner-after-native-drag: the generated `States::NativeDrag` is a unit
// variant: it carries no owner.
use impl_smlang::pointer::{Pointer, States};
use spec::pointer::{Config, Machine};

fn main() {
    let mut m = Pointer::new(Config::default());
    if let States::NativeDrag(owner) = m.state_enum() {
        let _ = owner;
    }
}
